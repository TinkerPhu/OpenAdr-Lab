mod common;
mod config;
mod controller;
mod entities;
mod models;
mod profile;
mod simulator;
mod state;
mod vtn;

use axum::{
    extract::{Path, Query, State},
    http::Method,
    response::IntoResponse,
    routing::{delete, get, post, put},
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
    sim: Arc<Mutex<SimState>>,
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
            // Track previous event IDs and tariff count for change detection (T034/T035)
            let mut prev_event_ids: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            let mut prev_tariff_count: usize = 0;
            let mut prev_import_limit: Option<f64> = None;
            loop {
                interval.tick().await;
                match vtn.fetch_events().await {
                    Ok(events) => {
                        counter!("poll_success_total", "resource" => "events").increment(1);
                        info!(resource = "events", count = events.len(), "poll success");

                        // Parse rates and capacity from new events (Stage 2)
                        let now = Utc::now();
                        let rates = controller::openadr_interface::parse_rate_snapshots(&events, now);
                        let new_cap = controller::openadr_interface::parse_capacity_state(&events);

                        // T034: Emit OpenAdrArrived / OpenAdrExpired for event set changes
                        let current_ids: std::collections::HashSet<String> = events
                            .iter()
                            .filter_map(|e| e.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
                            .collect();
                        for evt in &events {
                            if let Some(id) = evt.get("id").and_then(|v| v.as_str()) {
                                if !prev_event_ids.contains(id) {
                                    let name = evt
                                        .get("eventName")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or(id)
                                        .to_string();
                                    let (signal_type, value, interval_n) = evt
                                        .get("intervals")
                                        .and_then(|v| v.as_array())
                                        .and_then(|arr| arr.first())
                                        .and_then(|iv| iv.get("payloads"))
                                        .and_then(|v| v.as_array())
                                        .and_then(|arr| arr.first())
                                        .map(|p| {
                                            let sig = p
                                                .get("type")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("UNKNOWN")
                                                .to_string();
                                            let val = p
                                                .get("values")
                                                .and_then(|v| v.as_array())
                                                .and_then(|a| a.first())
                                                .and_then(|v| v.as_f64())
                                                .unwrap_or(0.0);
                                            let n = evt
                                                .get("intervals")
                                                .and_then(|v| v.as_array())
                                                .map(|a| a.len() as u32)
                                                .unwrap_or(0);
                                            (sig, val, n)
                                        })
                                        .unwrap_or_else(|| ("UNKNOWN".to_string(), 0.0, 0));
                                    state
                                        .push_controller_event(
                                            controller::trace::ControllerEvent::OpenAdrArrived {
                                                ts: now,
                                                event_name: name,
                                                signal_type,
                                                value,
                                                interval: interval_n,
                                            },
                                        )
                                        .await;
                                }
                            }
                        }
                        for old_id in &prev_event_ids {
                            if !current_ids.contains(old_id) {
                                state
                                    .push_controller_event(
                                        controller::trace::ControllerEvent::OpenAdrExpired {
                                            ts: now,
                                            event_name: old_id.clone(),
                                        },
                                    )
                                    .await;
                            }
                        }
                        prev_event_ids = current_ids;

                        // T035: Emit RateChange when tariff count changes
                        if !rates.is_empty() && rates.len() != prev_tariff_count {
                            if let Some(first) = rates.first() {
                                state
                                    .push_controller_event(
                                        controller::trace::ControllerEvent::RateChange {
                                            ts: now,
                                            interval_start: first.interval_start,
                                            import_eur_kwh: first
                                                .import_tariff_eur_kwh
                                                .unwrap_or(0.0),
                                            export_eur_kwh: first
                                                .export_tariff_eur_kwh
                                                .unwrap_or(0.0),
                                        },
                                    )
                                    .await;
                            }
                        }
                        prev_tariff_count = rates.len();

                        // T035: Emit CapacityChange when import limit changes
                        if new_cap.import_limit_kw != prev_import_limit {
                            state
                                .push_controller_event(
                                    controller::trace::ControllerEvent::CapacityChange {
                                        ts: now,
                                        import_limit_kw: new_cap.import_limit_kw,
                                        export_limit_kw: new_cap.export_limit_kw,
                                    },
                                )
                                .await;
                            prev_import_limit = new_cap.import_limit_kw;
                        }

                        state.set_planned_tariffs(rates).await;
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

    {
        let state = state.clone();
        let sim = sim_state.clone();
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

                // Pre-tick: snapshot plan/packets/capacity/tariffs for dispatcher
                let plan_snap = state.active_plan().await;
                let packets_snap = state.active_packets().await;
                let capacity_snap = state.capacity_state().await;
                let rates_snap = state.planned_tariffs().await;

                // Tick loop: build_setpoints → sim.tick → update_sim → accounting
                let sim_snapshot = {
                    let mut sim_guard = sim.lock().await;

                    // Build setpoints from plan (single authoritative control path)
                    let sp_map: std::collections::HashMap<String, f64> = match &plan_snap {
                        Some(ref plan) => controller::dispatcher::build_setpoints(
                            plan,
                            &sim_guard.assets,
                            &capacity_snap,
                            now,
                        ),
                        None => sim_guard
                            .assets
                            .iter()
                            .map(|a| (a.id.clone(), a.state.default_setpoint()))
                            .collect(),
                    };

                    // Simulator: apply setpoints → update device states
                    sim_guard.tick(dt_s, sp_map, now, &overrides);

                    // Update sensor snapshot (backward compat)
                    let sensor = sim_guard.to_sensor_snapshot();
                    state.update_sensor(sensor).await;

                    // Update sim in app state
                    let sim_snap = sim_guard.to_sim_snapshot();
                    state.update_sim(sim_snap.clone()).await;

                    // Post-tick: consolidated accounting — packets + ledger (T041/T043)
                    let mut updated_pkts = packets_snap.clone();
                    let mut ledger = state.asset_ledger().await;
                    let (trigger_opt, pkt_events) = controller::monitor::record_tick(
                        &mut updated_pkts,
                        &mut ledger,
                        &sim_snap,
                        &rates_snap,
                        dt_s,
                        now,
                    );
                    state.set_active_packets(updated_pkts).await;
                    state.set_asset_ledger(ledger).await;
                    // Push PacketTransition events + fire event-driven status reports (T042/T051)
                    for evt in pkt_events {
                        state.push_controller_event(evt.clone()).await;
                        // T051: status report for each PacketTransition
                        let trace = state.controller_trace().await;
                        if let Some(report) = controller::reporter::build_status_report(
                            &evt,
                            &trace.asset_history,
                            &ven_name,
                            now,
                        ) {
                            if let Err(e) = vtn_for_tick.upsert_report(report).await {
                                error!("status report (packet transition) submission failed: {e:#}");
                            }
                        }
                    }
                    if let Some(t) = trigger_opt {
                        let _ = trigger_tx_tick.send(t);
                    }

                    // Post-tick: push asset history rows (T032 — drives GET /trace/history)
                    {
                        let current_tariff = rates_snap.iter().find(|t| {
                            t.interval_start <= now && now < t.interval_end
                        });
                        let import_price = current_tariff
                            .and_then(|t| t.import_tariff_eur_kwh)
                            .unwrap_or(0.0);
                        let export_price = current_tariff
                            .and_then(|t| t.export_tariff_eur_kwh)
                            .unwrap_or(0.0);
                        let co2_g_kwh = current_tariff
                            .and_then(|t| t.co2_g_kwh)
                            .unwrap_or(0.0);

                        for (asset_id, asset_snap) in &sim_snap.assets {
                            let mut row: std::collections::HashMap<String, f64> =
                                std::collections::HashMap::new();
                            row.insert("power_kw".to_string(), asset_snap.power_kw);
                            for (k, v) in &asset_snap.values {
                                row.insert(k.clone(), *v);
                            }
                            let cost_rate_eur_h = if asset_snap.power_kw >= 0.0 {
                                asset_snap.power_kw * import_price
                            } else {
                                -asset_snap.power_kw * export_price
                            };
                            let co2_rate_g_h = if asset_snap.power_kw > 0.0 {
                                asset_snap.power_kw * co2_g_kwh
                            } else {
                                0.0
                            };
                            row.insert("cost_rate_eur_h".to_string(), cost_rate_eur_h);
                            row.insert("co2_rate_g_h".to_string(), co2_rate_g_h);
                            state.push_asset_row(asset_id, now, row).await;
                        }
                    }

                    sim_guard.clone()
                };

                let _ = sim_snapshot; // used by reporting in Phase 6

                // Periodic measurement reports (T049)
                if report_every_ticks > 0 {
                    report_counter += 1;
                    if report_counter >= report_every_ticks {
                        report_counter = 0;
                        let events = state.events().await;
                        let trace = state.controller_trace().await;
                        let reports =
                            controller::reporter::build_measurement_reports_for_active_events(
                                &events,
                                &trace.asset_history,
                                &ven_name,
                                now,
                            );
                        for report in reports {
                            if let Err(e) = vtn_for_tick.upsert_report(report).await {
                                error!("measurement report submission failed: {e:#}");
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
        let cfg_ven_name = cfg.ven_name.clone();
        let state_vtn = vtn.clone();
        let sim_for_planner = sim_state.clone();
        tokio::spawn(async move {
            // Initial delay: let event poll populate rates before first plan
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            loop {
                let now = Utc::now();
                let rates = state.planned_tariffs().await;
                let packets = state.active_packets().await;
                let capacity = state.capacity_state().await;
                let trigger = trigger_rx.borrow().clone();
                let trigger_reason = format!("{:?}", trigger);

                // Compute per-asset forecasts covering the planning horizon.
                let planning_horizon = chrono::Duration::seconds(
                    (profile.planner.plan_horizon_h * 3600) as i64,
                );
                let asset_forecasts: std::collections::HashMap<String, crate::common::QuantitySeries> = {
                    let sim_guard = sim_for_planner.lock().await;
                    sim_guard
                        .assets
                        .iter()
                        .map(|e| (e.id.clone(), e.state.forecast(planning_horizon)))
                        .collect()
                };

                let plan = controller::planner::run_planner(
                    &rates,
                    &packets,
                    &capacity,
                    &profile,
                    now,
                    trigger,
                    &asset_forecasts,
                );
                // Planner may transition packet statuses (Pending→Scheduled, etc.)
                let firm_count = plan.firm_slots.len();
                let flex_count = plan.flexible_slots.len();
                state.set_active_packets(plan.packets.clone()).await;
                state.set_active_plan(Some(plan)).await;
                info!("plan cycle complete");

                // Emit PlanCycle controller event (T029)
                let plan_cycle_event = controller::trace::ControllerEvent::PlanCycle {
                    ts: now,
                    trigger_reason,
                    firm_slots: firm_count,
                    flexible_slots: flex_count,
                };
                state.push_controller_event(plan_cycle_event.clone()).await;

                // Event-driven status report on PlanCycle (T050)
                {
                    let trace = state.controller_trace().await;
                    if let Some(report) = controller::reporter::build_status_report(
                        &plan_cycle_event,
                        &trace.asset_history,
                        &cfg_ven_name,
                        now,
                    ) {
                        if let Err(e) = state_vtn.upsert_report(report).await {
                            error!("status report (plan cycle) submission failed: {e:#}");
                        }
                    }
                }

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
        sim: sim_state.clone(),
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
        .route("/sim/schema", get(get_sim_schema))
        .route("/sim/reset/:asset_id", post(post_sim_reset))
        .route("/sim/config/battery", put(put_sim_config_battery))
        .route("/sim/override", get(get_sim_override).post(post_sim_override))
        .route("/trace/events", get(get_trace_events))
        .route("/trace/history", get(get_trace_history))
        .route("/metrics", get(get_metrics))
        // HEMS Stage 1–3 routes
        .route("/packets", get(get_packets).post(post_packets))
        .route("/plan", get(get_plan))
        .route("/tariffs", get(get_tariffs))
        // Timeline routes (speckit 005) — /all must precede /:asset_id
        .route("/timeline/all", get(get_timeline_all))
        .route("/timeline/:asset_id", get(get_timeline))
        // Asset forecast + history endpoints (speckit 007)
        .route("/forecast/:asset_id", get(get_asset_forecast))
        .route("/history/:asset_id", get(get_asset_history))
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

/// GET /sim/schema — returns control descriptors for all configured assets.
async fn get_sim_schema(State(ctx): State<AppCtx>) -> impl IntoResponse {
    let sim = ctx.sim.lock().await;
    let schema: std::collections::HashMap<String, Vec<simulator::assets::ControlDescriptor>> = sim
        .assets
        .iter()
        .map(|entry| (entry.id.clone(), entry.state.control_schema()))
        .collect();
    Json(schema)
}

#[derive(serde::Deserialize)]
struct SocBody {
    soc: f64,
}

/// POST /sim/reset/:asset_id — jump an asset's SoC to the given value.
async fn post_sim_reset(
    State(ctx): State<AppCtx>,
    Path(asset_id): Path<String>,
    Json(body): Json<SocBody>,
) -> impl IntoResponse {
    if !(0.0..=1.0).contains(&body.soc) {
        return (axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "soc must be between 0.0 and 1.0"}))).into_response();
    }
    let mut sim = ctx.sim.lock().await;
    match sim.asset_mut(&asset_id) {
        Some(entry) => {
            let mut values = std::collections::HashMap::new();
            values.insert("soc".to_string(), body.soc);
            entry.state.reset(values);
            drop(sim);
            axum::http::StatusCode::NO_CONTENT.into_response()
        }
        None => (axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("asset '{}' not found", asset_id)}))).into_response(),
    }
}

#[derive(serde::Deserialize)]
struct BatteryConfigBody {
    capacity_kwh: f64,
}

/// PUT /sim/config/battery — update battery capacity_kwh.
async fn put_sim_config_battery(
    State(ctx): State<AppCtx>,
    Json(body): Json<BatteryConfigBody>,
) -> impl IntoResponse {
    if body.capacity_kwh <= 0.0 {
        return (axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "capacity_kwh must be > 0"}))).into_response();
    }
    let mut sim = ctx.sim.lock().await;
    match sim.asset_mut("battery") {
        Some(entry) => {
            let mut values = std::collections::HashMap::new();
            values.insert("capacity_kwh".to_string(), body.capacity_kwh);
            entry.state.update_config(values);
            drop(sim);
            axum::http::StatusCode::NO_CONTENT.into_response()
        }
        None => (axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "battery asset not found"}))).into_response(),
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

#[derive(Deserialize)]
struct TraceHistoryQuery {
    asset: String,
    limit: Option<usize>,
}

/// GET /trace/events?limit=N — returns recent ControllerEvent log entries (newest first).
async fn get_trace_events(
    State(ctx): State<AppCtx>,
    Query(q): Query<TraceQuery>,
) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(50);
    let ct = ctx.state.controller_trace().await;
    let mut events = ct.events();
    events.reverse();
    events.truncate(limit);
    Json(events)
}

/// GET /trace/history?asset=<id>&limit=N — returns timeline rows for an asset.
async fn get_trace_history(
    State(ctx): State<AppCtx>,
    Query(q): Query<TraceHistoryQuery>,
) -> impl IntoResponse {
    let ct = ctx.state.controller_trace().await;
    let limit = q.limit.unwrap_or(100);
    match ct.asset_history_for(&q.asset) {
        None => Json(Vec::<serde_json::Value>::new()).into_response(),
        Some(buf) => {
            let mut points = buf.to_timeline(None);
            points.reverse();
            points.truncate(limit);
            let json: Vec<serde_json::Value> = points
                .into_iter()
                .map(|p| {
                    let mut m = serde_json::Map::new();
                    m.insert("ts".to_string(), serde_json::json!(p.ts));
                    for (k, v) in p.values {
                        m.insert(k, serde_json::json!(v));
                    }
                    serde_json::Value::Object(m)
                })
                .collect();
            Json(json).into_response()
        }
    }
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

/// GET /tariffs — returns planned tariff snapshots parsed from active events.
async fn get_tariffs(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.planned_tariffs().await)
}

// ─────────────────────────────────────────────────────────────────────────────
// Timeline endpoints (speckit 005)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct TimelineParams {
    hours_back: Option<f64>,
    hours_forward: Option<f64>,
    /// Maximum number of data points to return. If the raw series has more
    /// points, it is evenly downsampled. Default: 120 (2 min at 1s resolution).
    max_points: Option<usize>,
}

/// Downsample a sorted time series to at most `max_points` entries by
/// striding evenly through the series. The last point is always included.
fn downsample(points: Vec<serde_json::Value>, max_points: usize) -> Vec<serde_json::Value> {
    let n = points.len();
    if n <= max_points || max_points == 0 {
        return points;
    }
    let step = (n as f64 / max_points as f64).ceil() as usize;
    let mut result: Vec<serde_json::Value> = points
        .iter()
        .step_by(step)
        .cloned()
        .collect();
    // Always include the last point for accurate "now" boundary.
    if result.last() != points.last() {
        if let Some(last) = points.last() {
            result.push(last.clone());
        }
    }
    result
}

/// Serialize a Vec<AssetTimelinePoint> to `[{"ts": "...", "values": {...}}, ...]`
/// filtering out NaN values (JSON does not support NaN).
fn serialize_timeline(
    points: Vec<controller::trace::AssetTimelinePoint>,
) -> Vec<serde_json::Value> {
    points
        .into_iter()
        .map(|p| {
            let values: serde_json::Map<String, serde_json::Value> = p
                .values
                .into_iter()
                .filter(|(_, v)| !v.is_nan())
                .map(|(k, v)| (k, serde_json::json!(v)))
                .collect();
            serde_json::json!({ "ts": p.ts, "values": values })
        })
        .collect()
}

/// GET /timeline/:asset_id — merged past+future timeline for one asset.
async fn get_timeline(
    State(ctx): State<AppCtx>,
    Path(asset_id): Path<String>,
    Query(params): Query<TimelineParams>,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    use controller::timeline::{build_asset_timeline, TimeWindow};
    use std::collections::HashSet;

    let now = Utc::now();
    let hours_back = params.hours_back.unwrap_or(1.0);
    let hours_forward = params.hours_forward.unwrap_or(1.0);
    let max_points = params.max_points.unwrap_or(120);

    let ct = ctx.state.controller_trace().await;
    let plan = ctx.state.active_plan().await;

    // Build the set of known sim asset IDs from the current sim snapshot.
    let known_assets: HashSet<String> = {
        let sim = ctx.sim.lock().await;
        sim.assets.iter().map(|e| e.id.clone()).collect()
    };

    match build_asset_timeline(
        &asset_id,
        &known_assets,
        &ct.asset_history,
        plan.as_ref(),
        now,
        TimeWindow { hours_back, hours_forward },
    ) {
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("unknown asset: {}", asset_id) })),
        )
            .into_response(),
        Some(points) => Json(downsample(serialize_timeline(points), max_points)).into_response(),
    }
}

/// GET /timeline/all — merged timelines for all configured assets + "grid".
async fn get_timeline_all(
    State(ctx): State<AppCtx>,
    Query(params): Query<TimelineParams>,
) -> impl IntoResponse {
    use controller::timeline::{build_asset_timeline, TimeWindow};
    use std::collections::HashSet;

    let now = Utc::now();
    let hours_back = params.hours_back.unwrap_or(1.0);
    let hours_forward = params.hours_forward.unwrap_or(1.0);
    let max_points = params.max_points.unwrap_or(120);

    let ct = ctx.state.controller_trace().await;
    let plan = ctx.state.active_plan().await;

    let known_assets: HashSet<String> = {
        let sim = ctx.sim.lock().await;
        sim.assets.iter().map(|e| e.id.clone()).collect()
    };

    let window = TimeWindow { hours_back, hours_forward };
    let mut result: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();

    // All sim assets
    for asset_id in &known_assets {
        if let Some(points) = build_asset_timeline(
            asset_id,
            &known_assets,
            &ct.asset_history,
            plan.as_ref(),
            now,
            TimeWindow {
                hours_back: window.hours_back,
                hours_forward: window.hours_forward,
            },
        ) {
            result.insert(
                asset_id.clone(),
                serde_json::Value::Array(downsample(serialize_timeline(points), max_points)),
            );
        }
    }

    // Grid virtual asset
    if let Some(points) = build_asset_timeline(
        "grid",
        &known_assets,
        &ct.asset_history,
        plan.as_ref(),
        now,
        TimeWindow {
            hours_back: window.hours_back,
            hours_forward: window.hours_forward,
        },
    ) {
        result.insert(
            "grid".to_string(),
            serde_json::Value::Array(downsample(serialize_timeline(points), max_points)),
        );
    }

    Json(serde_json::Value::Object(result))
}

/// Query parameters for GET /forecast/:asset_id.
#[derive(Deserialize)]
struct ForecastParams {
    timespan_s: Option<f64>,
}

/// GET /forecast/:asset_id — forward-looking QuantitySeries for one asset (speckit 007).
/// Returns `{"samples": [{"ts": "...", "value": ...}], "quantity": "...", "unit": "...", "interpolation": "..."}`.
async fn get_asset_forecast(
    State(ctx): State<AppCtx>,
    Path(asset_id): Path<String>,
    Query(params): Query<ForecastParams>,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    use chrono::Duration;

    let timespan_s = params.timespan_s.unwrap_or(0.0);
    let timespan = Duration::milliseconds((timespan_s * 1000.0) as i64);

    let sim = ctx.sim.lock().await;
    let entry = sim.assets.iter().find(|e| e.id == asset_id);
    match entry {
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("unknown asset: {}", asset_id) })),
        )
            .into_response(),
        Some(entry) => {
            let series = entry.state.forecast(timespan);
            let samples: Vec<serde_json::Value> = series
                .samples
                .iter()
                .map(|(ts, v)| serde_json::json!({ "ts": ts, "value": v }))
                .collect();
            Json(serde_json::json!({
                "samples": samples,
                "quantity": series.quantity,
                "unit": series.unit,
                "interpolation": series.interpolation,
            }))
            .into_response()
        }
    }
}

/// Query parameters for GET /history/:asset_id.
#[derive(Deserialize)]
struct HistoryParams {
    timespan_s: Option<f64>,
}

/// GET /history/:asset_id — historical QuantitySeries for one asset (speckit 007).
/// Returns `{"samples": [{"ts": "...", "value": ...}], "quantity": "...", "unit": "...", "interpolation": "..."}`.
async fn get_asset_history(
    State(ctx): State<AppCtx>,
    Path(asset_id): Path<String>,
    Query(params): Query<HistoryParams>,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    use chrono::Duration;

    let timespan_s = params.timespan_s.unwrap_or(0.0);
    let timespan = Duration::milliseconds((timespan_s * 1000.0) as i64);

    let ct = ctx.state.controller_trace().await;
    let sim = ctx.sim.lock().await;
    let entry = sim.assets.iter().find(|e| e.id == asset_id);
    match entry {
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("unknown asset: {}", asset_id) })),
        )
            .into_response(),
        Some(entry) => {
            let history = ct.asset_history.get(&asset_id);
            let empty_buf = crate::controller::trace::AssetHistoryBuffer::new(0);
            let buf = history.unwrap_or(&empty_buf);
            let series = entry.state.past(timespan, buf);
            let samples: Vec<serde_json::Value> = series
                .samples
                .iter()
                .map(|(ts, v)| serde_json::json!({ "ts": ts, "value": v }))
                .collect();
            Json(serde_json::json!({
                "samples": samples,
                "quantity": series.quantity,
                "unit": series.unit,
                "interpolation": series.interpolation,
            }))
            .into_response()
        }
    }
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
    let assets = ctx.sim.lock().await.assets.clone();

    match controller::user_request::create_from_body(body, &assets, now) {
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
            // T044: emit RequestTransition for new request
            ctx.state.push_controller_event(
                controller::trace::ControllerEvent::RequestTransition {
                    ts: now,
                    request_id: user_req.id,
                    asset_id: user_req.asset_id.clone(),
                    from_status: "None".to_string(),
                    to_status: format!("{:?}", user_req.status),
                },
            ).await;
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
            // abandon_packet is now atomic inside cancel_request
            // T044: emit RequestTransition for cancellation
            ctx.state.push_controller_event(
                controller::trace::ControllerEvent::RequestTransition {
                    ts: Utc::now(),
                    request_id: id,
                    asset_id: String::new(),
                    from_status: "Active".to_string(),
                    to_status: "Cancelled".to_string(),
                },
            ).await;
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

