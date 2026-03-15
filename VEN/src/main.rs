mod config;
mod controller;
mod entities;
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
    routing::{delete, get, put},
    Json, Router,
};
use chrono::Utc;
use entities::asset::{ComfortRate, CompletionPolicy, PlanTrigger, UserRequestMode};
use controller::user_request::CreateUserRequestBody;
use entities::energy_packet::{DeadlineTier, EnergyPacket, PacketStatus, ValueCurve};
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
    trigger_tx: Arc<tokio::sync::watch::Sender<PlanTrigger>>,
    profile: Arc<Profile>,
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

    // PlanTrigger watch channel — event poll and dispatcher send triggers;
    // planning loop receives them for reactive replanning.
    let (trigger_tx, trigger_rx) = tokio::sync::watch::channel(PlanTrigger::Periodic);
    let trigger_tx = Arc::new(trigger_tx);

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

    // Seed EnergyPackets from profile (Stage 3)
    {
        let now = Utc::now();
        let seeded = controller::planner::seed_packets_from_profile(&profile, now);
        if !seeded.is_empty() {
            info!(count = seeded.len(), "seeded EnergyPackets from profile");
            state.set_active_packets(seeded).await;
        }
    }

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
        let trigger_tx_events = trigger_tx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(secs));
            loop {
                interval.tick().await;
                match vtn.fetch_events().await {
                    Ok(events) => {
                        counter!("poll_success_total", "resource" => "events").increment(1);
                        info!(resource = "events", count = events.len(), "poll success");

                        // Parse rates and capacity from new events (Stage 2)
                        let now = Utc::now();
                        let rates = controller::openadr_interface::parse_rate_snapshots(&events);
                        state.set_planned_rates(rates).await;

                        let new_cap = controller::openadr_interface::parse_capacity_state(&events);
                        state.set_capacity_state(new_cap).await;

                        let existing_obs = state.obligations().await;
                        let new_obs = controller::openadr_interface::extract_report_obligations(
                            &events, now, &existing_obs,
                        );
                        state.add_obligations(new_obs).await;

                        state.set_events(events, 500).await;

                        // Signal planner: rates may have changed
                        let _ = trigger_tx_events.send(PlanTrigger::RateChange);
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
        let trigger_tx_tick = trigger_tx.clone();
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

                // Pre-tick: snapshot plan/packets/rates for dispatcher (no lock needed)
                let plan_snap = state.active_plan().await;
                let packets_snap = state.active_packets().await;
                let rates_snap = state.planned_rates().await;

                // Reactor: evaluate events → setpoints; dispatcher overlays plan allocations
                let (setpoints, sim_snapshot, reactor_mode) = {
                    let mut sim_guard = sim.lock().await;
                    let mut reactor_guard = reactor.lock().await;
                    let mut setpoints = reactor_guard.evaluate(&events, &sim_guard, &profile, now, dt_s, &overrides);

                    // Dispatcher overlay: HEMS plan allocations override reactor for managed assets
                    if let Some(ref plan) = plan_snap {
                        let disp = controller::dispatcher::get_setpoints(plan, &packets_snap, now);
                        if let Some(kw) = disp.ev_kw      { setpoints.ev_charge_kw  = kw; }
                        if let Some(kw) = disp.battery_kw { setpoints.battery_kw    = kw; }
                        if let Some(kw) = disp.heater_kw  { setpoints.heater_kw     = kw; }
                    }

                    // Owner force-overrides beat everything (including dispatcher)
                    if let Some(kw) = overrides.ev_force_kw       { setpoints.ev_charge_kw   = kw; }
                    if let Some(kw) = overrides.heater_force_kw   { setpoints.heater_kw       = kw; }
                    if let Some(kw) = overrides.battery_force_kw  { setpoints.battery_kw      = kw; }
                    if let Some(limit) = overrides.pv_force_export_limit_kw { setpoints.pv_export_limit_kw = Some(limit); }

                    // Simulator: apply setpoints → update device states
                    sim_guard.tick(dt_s, &setpoints, now, &overrides);

                    // Update sensor snapshot (backward compat)
                    let sensor = sim_guard.to_sensor_snapshot();
                    state.update_sensor(sensor).await;

                    // Update sim + trace in app state
                    let sim_snap = sim_guard.to_sim_snapshot();
                    let trace = reactor_guard.trace_entries();
                    state.update_sim(sim_snap.clone(), trace).await;

                    // Post-tick: accumulate actual energy into packets and transition statuses
                    let mut updated_pkts = packets_snap.clone();
                    let trigger_opt =
                        controller::dispatcher::update_packets(&mut updated_pkts, &sim_snap, dt_s, now);
                    state.set_active_packets(updated_pkts).await;
                    if let Some(t) = trigger_opt {
                        let _ = trigger_tx_tick.send(t);
                    }

                    // Post-tick: update asset energy ledger
                    let mut ledger = state.asset_ledger().await;
                    controller::monitor::update_ledger(&mut ledger, &sim_snap, &rates_snap, dt_s, now);
                    state.set_asset_ledger(ledger).await;

                    // Snapshot sim state for reporting (avoid holding lock during HTTP)
                    let sim_clone = sim_guard.clone();
                    let mode = setpoints.mode.clone();

                    (setpoints, sim_clone, mode)
                };

                let _ = setpoints; // used above, suppress warning

                // Clear one-shot stub fields (ev_initial_soc, battery_initial_soc) after tick applied them
                if overrides.ev_initial_soc.is_some() || overrides.battery_initial_soc.is_some() {
                    let mut cleared = overrides.clone();
                    cleared.ev_initial_soc = None;
                    cleared.battery_initial_soc = None;
                    state.set_overrides(cleared).await;
                }

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

    // Obligation check loop (every 5s — Stage 2)
    {
        let state = state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
            loop {
                interval.tick().await;
                let now = Utc::now();
                let due = state.due_obligations(now).await;
                for ob in due {
                    // Stage 2: mark fulfilled; actual report building is reporter.rs work
                    state.mark_obligation_fulfilled(ob.id).await;
                    info!(
                        obligation_id = %ob.id,
                        payload_type = %ob.payload_type,
                        "obligation fulfilled (stub)"
                    );
                }
            }
        });
    }

    // Planning loop (Stage 4) — reactive: runs on trigger OR periodic timer
    {
        let state = state.clone();
        let profile = profile.clone();
        let replan_s = profile.planner.replan_interval_s;
        let mut trigger_rx = trigger_rx;
        tokio::spawn(async move {
            // Initial delay: let event poll populate rates before first plan
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            loop {
                let now = Utc::now();
                let rates = state.planned_rates().await;
                let packets = state.active_packets().await;
                let capacity = state.capacity_state().await;
                let trigger = trigger_rx.borrow().clone();
                let plan = controller::planner::run_planner(
                    &rates,
                    &packets,
                    &capacity,
                    &profile,
                    now,
                    trigger,
                );
                // Planner may transition packet statuses (Pending→Scheduled, etc.)
                state.set_active_packets(plan.packets.clone()).await;
                state.set_active_plan(Some(plan)).await;
                info!("plan cycle complete");

                // Wait for next trigger OR periodic timeout (whichever comes first)
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(replan_s)) => {}
                    _ = trigger_rx.changed() => {}
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
        trigger_tx,
        profile: profile.clone(),
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
        // HEMS Stage 1–3 routes
        .route("/packets", get(get_packets).post(post_packets))
        .route("/plan", get(get_plan))
        .route("/rates", get(get_rates))
        // HEMS Stage 2 routes
        .route("/capacity", get(get_capacity))
        .route("/obligations", get(get_obligations))
        // HEMS Stage 4 routes
        .route("/ledger", get(get_ledger))
        // HEMS Stage 5 routes
        .route("/user-requests", get(get_requests).post(post_requests))
        .route("/user-requests/:id", delete(delete_request))
        .route("/flexibility", get(get_flexibility))
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

// --- HEMS stub routes (Stage 1) ---

/// GET /packets — returns active EnergyPackets (empty until Stage 3).
async fn get_packets(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.active_packets().await)
}

/// GET /plan — returns the active Plan (null until Stage 3).
async fn get_plan(State(ctx): State<AppCtx>) -> impl IntoResponse {
    match ctx.state.active_plan().await {
        Some(plan) => Json(serde_json::to_value(plan).unwrap_or_default()).into_response(),
        None => Json(serde_json::Value::Null).into_response(),
    }
}

/// GET /rates — returns planned rate snapshots parsed from active events (Stage 2).
async fn get_rates(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.planned_rates().await)
}

/// GET /capacity — returns the current OadrCapacityState (Stage 2).
async fn get_capacity(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.capacity_state().await)
}

/// GET /obligations — returns pending report obligations (Stage 2).
async fn get_obligations(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.obligations().await)
}

/// POST /packets body shape (Stage 4).
#[derive(Deserialize)]
struct CreatePacketRequest {
    asset_id: String,
    target_energy_kwh: Option<f64>,
    target_soc: Option<f64>,
    desired_power_kw: Option<f64>,
    latest_end: Option<chrono::DateTime<Utc>>,
}

/// POST /packets — create a new EnergyPacket and trigger a replan (Stage 4).
async fn post_packets(
    State(ctx): State<AppCtx>,
    Json(body): Json<CreatePacketRequest>,
) -> impl IntoResponse {
    let now = Utc::now();
    let desired_power_kw = body.desired_power_kw.unwrap_or(1.0);
    let target_energy_kwh = body.target_energy_kwh.unwrap_or(desired_power_kw); // default: 1h

    let packet = EnergyPacket {
        id: Uuid::new_v4(),
        asset_id: body.asset_id,
        status: PacketStatus::Pending,
        earliest_start: now,
        latest_start: None,
        target_energy_kwh,
        target_soc: body.target_soc,
        desired_power_kw,
        value_curve: ValueCurve {
            comfort_rates: vec![
                ComfortRate { fill: 0.0, max_marginal_price: 0.35, max_marginal_co2: 0.0 },
                ComfortRate { fill: 1.0, max_marginal_price: 0.05, max_marginal_co2: 0.0 },
            ],
            deadline_tiers: body
                .latest_end
                .map(|le| {
                    vec![DeadlineTier {
                        deadline: le,
                        max_total_cost_eur: None,
                        max_marginal_rate_eur_kwh: None,
                        min_completion: 0.8,
                    }]
                })
                .unwrap_or_default(),
            active_tier_index: 0,
        },
        request_mode: UserRequestMode::ByDeadline,
        completion_policy: CompletionPolicy::Stop,
        post_deadline_comfort_bid: None,
        planned_power_profile: vec![],
        past_power_profile: vec![],
        accumulated_cost_eur: 0.0,
        accumulated_co2_g: 0.0,
        estimated_cost_eur: 0.0,
        estimated_co2_g: 0.0,
        estimated_completion: 0.0,
        last_estimate_at: None,
        created_at: now,
        updated_at: now,
    };

    let mut packets = ctx.state.active_packets().await;
    packets.push(packet.clone());
    ctx.state.set_active_packets(packets).await;

    // Signal the planning loop: a new packet needs scheduling
    let _ = ctx.trigger_tx.send(PlanTrigger::UserRequest);

    info!(asset_id = %packet.asset_id, packet_id = %packet.id, "new EnergyPacket created via POST /packets");
    (axum::http::StatusCode::CREATED, Json(packet))
}

/// GET /ledger — returns per-asset cumulative energy/cost/CO₂ (Stage 4).
async fn get_ledger(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.asset_ledger().await)
}

// --- Stage 5: User Request Manager ---

/// GET /user-requests — list all user requests.
async fn get_requests(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.active_requests().await)
}

/// POST /user-requests — create a user energy task request (Stage 5).
async fn post_requests(
    State(ctx): State<AppCtx>,
    Json(body): Json<CreateUserRequestBody>,
) -> impl IntoResponse {
    let now = Utc::now();
    let sim = ctx.state.sim().await;

    match controller::user_request::create_from_body(body, &ctx.profile, sim.as_ref(), now) {
        Ok((user_req, packet)) => {
            info!(
                request_id = %user_req.id,
                packet_id = %packet.id,
                asset_id = %packet.asset_id,
                target_kwh = packet.target_energy_kwh,
                "user request created"
            );
            let mut packets = ctx.state.active_packets().await;
            packets.push(packet);
            ctx.state.set_active_packets(packets).await;
            ctx.state.upsert_request(user_req.clone()).await;
            let _ = ctx.trigger_tx.send(PlanTrigger::UserRequest);
            (axum::http::StatusCode::CREATED, Json(serde_json::to_value(user_req).unwrap_or_default())).into_response()
        }
        Err(e) => {
            warn!("POST /user-requests rejected: {e}");
            (
                axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        }
    }
}

/// DELETE /user-requests/:id — cancel a user request and abandon its packet (Stage 5).
async fn delete_request(
    State(ctx): State<AppCtx>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match ctx.state.cancel_request(id).await {
        Some(packet_id) => {
            ctx.state.abandon_packet(packet_id).await;
            let _ = ctx.trigger_tx.send(PlanTrigger::UserRequest);
            info!(request_id = %id, packet_id = %packet_id, "user request cancelled");
            axum::http::StatusCode::NO_CONTENT.into_response()
        }
        None => (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "request not found"})),
        )
            .into_response(),
    }
}

/// GET /flexibility — returns FlexibilityEnvelopes from the active plan (Stage 5).
async fn get_flexibility(State(ctx): State<AppCtx>) -> impl IntoResponse {
    match ctx.state.active_plan().await {
        Some(plan) => Json(serde_json::to_value(plan.envelopes).unwrap_or_default()).into_response(),
        None => Json(serde_json::json!([])).into_response(),
    }
}

