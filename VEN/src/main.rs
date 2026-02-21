mod config;
mod models;
mod profile;
mod reactor;
mod reporter;
mod simulator;
mod state;
mod vtn;

use axum::{
    extract::{Path, Query, State},
    http::Method,
    response::IntoResponse,
    routing::{get, put},
    Json, Router,
};
use chrono::Utc;
use config::Config;
use metrics::counter;
use metrics_exporter_prometheus::PrometheusBuilder;
use models::{SensorInput, SensorSnapshot};
use profile::Profile;
use reactor::Reactor;
use serde::Deserialize;
use simulator::SimState;
use state::{AppState, UserOverrides};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{error, info, warn};
use uuid::Uuid;
use vtn::VtnClient;

#[derive(Clone)]
struct AppCtx {
    state: AppState,
    vtn: VtnClient,
    metrics_handle: Arc<metrics_exporter_prometheus::PrometheusHandle>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()))
        .init();

    let metrics_handle = PrometheusBuilder::new().install_recorder()?;

    let cfg = Config::from_env()?;
    info!("starting ven {} listening on {}", cfg.ven_name, cfg.listen_addr);

    let state = AppState::new();

    // Optional: load persisted state
    if let Some(path) = cfg.persist_path.clone() {
        if let Ok(s) = tokio::fs::read_to_string(&path).await {
            if let Err(e) = state.load_from_json(&s).await {
                error!("failed to load persisted state: {e:#}");
            } else {
                info!("loaded persisted state from {path}");
            }
        }
    }

    // Load simulator profile
    let profile = if let Some(ref path) = cfg.profile_path {
        Profile::load(path).await
    } else {
        warn!("PROFILE_PATH not set, using default profile");
        Profile::default()
    };
    let profile = Arc::new(profile);

    let vtn = VtnClient::new(cfg.vtn_base_url.clone(), cfg.client_id.clone(), cfg.client_secret.clone(), cfg.ven_name.clone());

    // Poll programs
    {
        let state = state.clone();
        let vtn = vtn.clone();
        let secs = cfg.poll_programs_secs;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(secs));
            loop {
                interval.tick().await;
                match vtn.fetch_programs().await {
                    Ok(programs) => {
                        counter!("poll_success_total", "resource" => "programs").increment(1);
                        info!(resource = "programs", count = programs.len(), "poll success");
                        state.set_programs(programs).await;
                    }
                    Err(e) => {
                        counter!("poll_error_total", "resource" => "programs").increment(1);
                        error!(resource = "programs", "poll failed: {e:#}");
                    }
                }
            }
        });
    }

    // Poll events
    {
        let state = state.clone();
        let vtn = vtn.clone();
        let secs = cfg.poll_events_secs;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(secs));
            loop {
                interval.tick().await;
                match vtn.fetch_events().await {
                    Ok(events) => {
                        counter!("poll_success_total", "resource" => "events").increment(1);
                        info!(resource = "events", count = events.len(), "poll success");
                        state.set_events(events, 500).await;
                    }
                    Err(e) => {
                        counter!("poll_error_total", "resource" => "events").increment(1);
                        error!(resource = "events", "poll failed: {e:#}");
                    }
                }
            }
        });
    }

    // Poll reports
    {
        let state = state.clone();
        let vtn = vtn.clone();
        let secs = cfg.poll_reports_secs;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(secs));
            loop {
                interval.tick().await;
                match vtn.fetch_reports().await {
                    Ok(reports) => {
                        counter!("poll_success_total", "resource" => "reports").increment(1);
                        info!(resource = "reports", count = reports.len(), "poll success");
                        state.set_reports(reports).await;
                    }
                    Err(e) => {
                        counter!("poll_error_total", "resource" => "reports").increment(1);
                        error!(resource = "reports", "poll failed: {e:#}");
                    }
                }
            }
        });
    }

    // Simulator + Reactor tick loop (replaces fake sensor)
    let sim_state = {
        // Try to load persisted sim state
        let data_dir = cfg.persist_path.as_deref()
            .and_then(|p| std::path::Path::new(p).parent())
            .and_then(|p| p.to_str())
            .unwrap_or("/data");

        let loaded = simulator::persist::load(data_dir).await;
        let sim = loaded.unwrap_or_else(|| SimState::from_profile(&profile));
        Arc::new(Mutex::new(sim))
    };

    let reactor_state = Arc::new(Mutex::new(Reactor::new()));

    {
        let state = state.clone();
        let sim = sim_state.clone();
        let reactor = reactor_state.clone();
        let profile = profile.clone();
        let tick_s = profile.simulator.tick_s;
        let persist_every_s = profile.simulator.persist_every_s;
        let report_interval_s = profile.simulator.report_interval_s;
        let ven_name = cfg.ven_name.clone();
        let vtn_for_tick = vtn.clone();
        let data_dir = cfg.persist_path.as_deref()
            .and_then(|p| std::path::Path::new(p).parent())
            .and_then(|p| p.to_str())
            .unwrap_or("/data")
            .to_string();

        tokio::spawn(async move {
            let mut tick_interval = tokio::time::interval(std::time::Duration::from_secs(tick_s));
            let mut persist_counter: u64 = 0;
            let persist_every_ticks = if tick_s > 0 { persist_every_s / tick_s } else { 15 };
            let mut report_counter: u64 = 0;
            let report_every_ticks = if tick_s > 0 && report_interval_s > 0 {
                report_interval_s / tick_s
            } else {
                0
            };

            loop {
                tick_interval.tick().await;
                let now = Utc::now();
                let dt_s = tick_s as f64;

                // Get current events and user overrides
                let events = state.events().await;
                let overrides = state.overrides().await;

                // Reactor: evaluate events → setpoints
                let (setpoints, sim_snapshot, reactor_mode) = {
                    let mut sim_guard = sim.lock().await;
                    let mut reactor_guard = reactor.lock().await;
                    let mut setpoints = reactor_guard.evaluate(&events, &sim_guard, &profile, now, dt_s, &overrides);

                    // Owner force-overrides applied AFTER reactor (trace records VTN intent unaffected)
                    if let Some(kw) = overrides.ev_force_kw       { setpoints.ev_charge_kw   = kw; }
                    if let Some(kw) = overrides.heater_force_kw   { setpoints.heater_kw       = kw; }
                    if let Some(limit) = overrides.pv_force_export_limit_kw { setpoints.pv_export_limit_kw = Some(limit); }

                    // Simulator: apply setpoints → update device states
                    sim_guard.tick(dt_s, &setpoints, now, &overrides);

                    // Update sensor snapshot (backward compat)
                    let sensor = sim_guard.to_sensor_snapshot();
                    state.update_sensor(sensor).await;

                    // Update sim + trace in app state
                    let sim_snap = sim_guard.to_sim_snapshot();
                    let trace = reactor_guard.trace_entries();
                    state.update_sim(sim_snap, trace).await;

                    // Snapshot sim state for reporting (avoid holding lock during HTTP)
                    let sim_clone = sim_guard.clone();
                    let mode = setpoints.mode.clone();

                    (setpoints, sim_clone, mode)
                };

                let _ = setpoints; // used above, suppress warning

                // Periodic auto-report submission
                if report_every_ticks > 0 {
                    report_counter += 1;
                    if report_counter >= report_every_ticks {
                        report_counter = 0;
                        let reports = reporter::build_reports_for_active_events(
                            &events, &sim_snapshot, &reactor_mode, &ven_name, now,
                        );
                        for report in reports {
                            let report_name = report.get("reportName")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown")
                                .to_string();
                            match vtn_for_tick.upsert_report(report).await {
                                Ok(_) => {
                                    counter!("auto_reports_sent_total").increment(1);
                                    info!(report_name, "auto-report submitted");
                                }
                                Err(e) => {
                                    counter!("auto_reports_error_total").increment(1);
                                    error!(report_name, "auto-report failed: {e:#}");
                                }
                            }
                        }
                    }
                }

                // Periodic persist
                persist_counter += 1;
                if persist_counter >= persist_every_ticks {
                    persist_counter = 0;
                    let sim_guard = sim.lock().await;
                    if let Err(e) = simulator::persist::save(&sim_guard, &data_dir).await {
                        error!("sim persist failed: {e:#}");
                    }
                }
            }
        });
    }

    // Optional persistence task for main app state
    if let Some(path) = cfg.persist_path.clone() {
        let state = state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(15));
            loop {
                interval.tick().await;
                match state.to_json().await {
                    Ok(json) => {
                        if let Err(e) = tokio::fs::write(&path, json).await {
                            error!("persist write failed: {e:#}");
                        }
                    }
                    Err(e) => error!("persist serialization failed: {e:#}"),
                }
            }
        });
    }

    // HTTP API
    let ctx = AppCtx {
        state,
        vtn,
        metrics_handle: Arc::new(metrics_handle),
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::PUT])
        .allow_headers(Any);

    let app = Router::new()
        .route("/health", get(health))
        .route("/events", get(get_events))
        .route("/programs", get(get_programs))
        .route("/sensors", get(get_sensors).post(post_sensors))
        .route("/reports", get(get_reports).post(post_reports))
        .route("/reports/:id", put(put_report))
        .route("/sim", get(get_sim))
        .route("/sim/override", get(get_sim_override).post(post_sim_override))
        .route("/trace", get(get_trace))
        .route("/metrics", get(get_metrics))
        .with_state(ctx)
        .layer(TraceLayer::new_for_http())
        .layer(cors);

    // Graceful shutdown: persist sim state on SIGTERM
    let sim_for_shutdown = sim_state.clone();
    let data_dir_for_shutdown = cfg.persist_path.as_deref()
        .and_then(|p| std::path::Path::new(p).parent())
        .and_then(|p| p.to_str())
        .unwrap_or("/data")
        .to_string();

    let listener = tokio::net::TcpListener::bind(&cfg.listen_addr).await?;
    info!("listening on {}", cfg.listen_addr);

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            info!("shutdown signal received, persisting sim state");
            let sim_guard = sim_for_shutdown.lock().await;
            if let Err(e) = simulator::persist::save(&sim_guard, &data_dir_for_shutdown).await {
                error!("shutdown persist failed: {e:#}");
            } else {
                info!("sim state persisted on shutdown");
            }
        })
        .await?;

    Ok(())
}

async fn health() -> &'static str {
    "ok"
}

async fn get_metrics(State(ctx): State<AppCtx>) -> impl IntoResponse {
    ctx.metrics_handle.render()
}

#[derive(Deserialize)]
struct EventsQuery {
    limit: Option<usize>,
}

async fn get_events(
    State(ctx): State<AppCtx>,
    Query(q): Query<EventsQuery>,
) -> impl IntoResponse {
    let mut events = ctx.state.events().await;
    let limit = q.limit.unwrap_or(100);
    if events.len() > limit {
        events.truncate(limit);
    }
    Json(events)
}

async fn get_programs(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.programs().await)
}

async fn get_sensors(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.sensor().await)
}

async fn post_sensors(
    State(ctx): State<AppCtx>,
    Json(input): Json<SensorInput>,
) -> impl IntoResponse {
    let snap = SensorSnapshot {
        id: Uuid::new_v4(),
        ts: Utc::now(),
        temperature_c: input.temperature_c,
        power_w: input.power_w,
        voltage_v: input.voltage_v,
        raw: input.raw.unwrap_or(serde_json::json!({})),
    };
    ctx.state.update_sensor(snap.clone()).await;
    Json(snap)
}

async fn get_reports(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.reports().await)
}

async fn post_reports(
    State(ctx): State<AppCtx>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    match ctx.vtn.upsert_report(body).await {
        Ok(result) => {
            counter!("reports_sent_total").increment(1);
            (axum::http::StatusCode::CREATED, Json(result)).into_response()
        }
        Err(e) => {
            error!("report submission failed: {e:#}");
            (
                axum::http::StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": format!("{e:#}")})),
            )
                .into_response()
        }
    }
}

async fn put_report(
    State(ctx): State<AppCtx>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    match ctx.vtn.update_report(&id, body).await {
        Ok(result) => {
            counter!("reports_sent_total").increment(1);
            (axum::http::StatusCode::OK, Json(result)).into_response()
        }
        Err(e) => {
            error!("report update failed: {e:#}");
            (
                axum::http::StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": format!("{e:#}")})),
            )
                .into_response()
        }
    }
}

async fn get_sim(State(ctx): State<AppCtx>) -> impl IntoResponse {
    match ctx.state.sim().await {
        Some(sim) => Json(serde_json::to_value(sim).unwrap_or_default()).into_response(),
        None => (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "simulator not yet initialized"})),
        )
            .into_response(),
    }
}

async fn get_sim_override(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.overrides().await)
}

async fn post_sim_override(
    State(ctx): State<AppCtx>,
    Json(body): Json<UserOverrides>,
) -> impl IntoResponse {
    ctx.state.set_overrides(body).await;
    axum::http::StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
struct TraceQuery {
    limit: Option<usize>,
}

async fn get_trace(
    State(ctx): State<AppCtx>,
    Query(q): Query<TraceQuery>,
) -> impl IntoResponse {
    let mut trace = ctx.state.trace().await;
    let limit = q.limit.unwrap_or(50);
    // Return newest first
    trace.reverse();
    trace.truncate(limit);
    Json(trace)
}
