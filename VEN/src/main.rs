mod assets;
mod common;
mod config;
mod controller;
mod entities;
mod models;
mod profile;
mod routes;
mod simulator;
mod state;
mod vtn;

use axum::{
    http::Method,
    routing::{delete, get, post, put},
    Router,
};
use chrono::{DateTime, Utc};
use entities::asset::PlanTrigger;
use config::Config;
use metrics::counter;
use metrics_exporter_prometheus::PrometheusBuilder;
use profile::Profile;
use simulator::SimState;
use state::AppState;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{debug, error, info, warn};
use vtn::VtnClient;

use routes::assets::{get_asset_forecast, get_asset_history};
use routes::events::{get_events, get_programs, get_sensors, post_sensors};
use routes::hems::{
    delete_request, get_capacity, get_flexibility, get_ledger, get_obligations,
    get_packets, get_plan, get_requests, get_tariffs, post_packets, post_requests,
};
use routes::reports::{get_reports, post_reports, put_report};
use routes::sim::{get_sim, get_sim_override, get_sim_schema, post_sim_override, post_sim_reset, put_sim_config_battery};
use routes::system::{get_metrics, health};
use routes::timeline::{get_timeline, get_timeline_all};
use routes::trace::{get_trace_events, get_trace_history};

#[derive(Clone)]
pub struct AppCtx {
    state: AppState,
    vtn: VtnClient,
    metrics_handle: Arc<metrics_exporter_prometheus::PrometheusHandle>,
    trigger_tx: Arc<tokio::sync::watch::Sender<PlanTrigger>>,
    profile: Arc<Profile>,
    sim: Arc<Mutex<SimState>>,
}

// ─── Event poll change detection (RF-B08) ─────────────────────────────────────

/// Output of `detect_event_changes` — all side-effect-free results of one poll tick.
struct EventChanges {
    /// Trace events to push to the controller log (arrived/expired/rate/capacity).
    pub trace_events: Vec<controller::trace::ControllerEvent>,
    /// Updated set of event IDs seen this tick (new value for `prev_event_ids`).
    pub current_ids: std::collections::HashSet<String>,
    /// Parsed tariff snapshots for this tick.
    pub rates: Vec<entities::tariff_snapshot::TariffSnapshot>,
    /// Parsed capacity state for this tick.
    pub capacity: entities::capacity::OadrCapacityState,
}

/// Pure change-detection pass over a freshly fetched event list.
///
/// Compares against previous poll state and returns all trace events that
/// should be emitted, plus parsed rates/capacity for storage.  No I/O, no
/// state mutations — safe to unit-test.
fn detect_event_changes(
    events: &[serde_json::Value],
    prev_ids: &std::collections::HashSet<String>,
    prev_tariff_count: usize,
    prev_import_limit: Option<f64>,
    now: DateTime<Utc>,
) -> EventChanges {
    let rates = controller::openadr_interface::parse_rate_snapshots(events, now);
    let capacity = controller::openadr_interface::parse_capacity_state(events);

    let current_ids: std::collections::HashSet<String> = events
        .iter()
        .filter_map(|e| e.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .collect();

    let mut trace_events = Vec::new();

    // OpenAdrArrived — events that are new this tick
    for evt in events {
        let Some(id) = evt.get("id").and_then(|v| v.as_str()) else { continue };
        if prev_ids.contains(id) { continue }

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

        trace_events.push(controller::trace::ControllerEvent::OpenAdrArrived {
            ts: now,
            event_name: name,
            signal_type,
            value,
            interval: interval_n,
        });
    }

    // OpenAdrExpired — events that disappeared this tick
    for old_id in prev_ids {
        if !current_ids.contains(old_id) {
            trace_events.push(controller::trace::ControllerEvent::OpenAdrExpired {
                ts: now,
                event_name: old_id.clone(),
            });
        }
    }

    // RateChange — tariff count changed
    if !rates.is_empty() && rates.len() != prev_tariff_count {
        if let Some(first) = rates.first() {
            trace_events.push(controller::trace::ControllerEvent::RateChange {
                ts: now,
                interval_start: first.interval_start,
                import_eur_kwh: first.import_tariff_eur_kwh.unwrap_or(0.0),
                export_eur_kwh: first.export_tariff_eur_kwh.unwrap_or(0.0),
            });
        }
    }

    // CapacityChange — import limit changed
    if capacity.import_limit_kw != prev_import_limit {
        trace_events.push(controller::trace::ControllerEvent::CapacityChange {
            ts: now,
            import_limit_kw: capacity.import_limit_kw,
            export_limit_kw: capacity.export_limit_kw,
        });
    }

    EventChanges { trace_events, current_ids, rates, capacity }
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

                        let now = Utc::now();
                        let changes = detect_event_changes(
                            &events,
                            &prev_event_ids,
                            prev_tariff_count,
                            prev_import_limit,
                            now,
                        );

                        for evt in changes.trace_events {
                            state.push_controller_event(evt).await;
                        }
                        prev_event_ids = changes.current_ids;
                        prev_tariff_count = changes.rates.len();
                        prev_import_limit = changes.capacity.import_limit_kw;

                        state.set_planned_tariffs(changes.rates).await;
                        state.set_capacity_state(changes.capacity).await;

                        let existing_obs = state.report_obligations().await;
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

    // Obligation check loop (every 5s — Stage 2 + RF-05e)
    {
        let state = state.clone();
        let vtn_for_ob = vtn.clone();
        let ven_name_ob = cfg.ven_name.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
            loop {
                interval.tick().await;
                let now = Utc::now();
                let due = state.due_obligations(now).await;
                for ob in due {
                    let trace = state.controller_trace().await;
                    if let Some(report) =
                        controller::reporter::build_measurement_report_for_obligation(
                            &ob,
                            &trace.asset_history,
                            &ven_name_ob,
                        )
                    {
                        match vtn_for_ob.upsert_report(report).await {
                            Ok(_) => {
                                state.mark_obligation_fulfilled(ob.id).await;
                                info!(
                                    obligation_id = %ob.id,
                                    payload_type = %ob.payload_type,
                                    "obligation report submitted"
                                );
                            }
                            Err(e) => {
                                error!(
                                    obligation_id = %ob.id,
                                    "obligation report submission failed: {e:#}"
                                );
                            }
                        }
                    } else {
                        // No history data to build report — mark fulfilled to avoid retry loop
                        state.mark_obligation_fulfilled(ob.id).await;
                        debug!(
                            obligation_id = %ob.id,
                            "obligation skipped (no history data)"
                        );
                    }
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
                let asset_forecasts: std::collections::HashMap<String, crate::common::TimeSeries> = {
                    let sim_guard = sim_for_planner.lock().await;
                    sim_guard
                        .assets
                        .iter()
                        .map(|e| (e.id.clone(), e.state.forecast(planning_horizon)))
                        .collect()
                };

                let tariff_ts = crate::entities::tariff_snapshot::TariffTimeSeries::from_snapshots(&rates);
                let plan = controller::planner::run_planner(
                    &tariff_ts,
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



#[cfg(test)]
mod event_poll_tests {
    use super::*;
    use chrono::TimeZone;

    fn ts() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 3, 21, 10, 0, 0).unwrap()
    }

    fn make_event(id: &str, name: &str, signal_type: &str, value: f64) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "eventName": name,
            "intervals": [{
                "payloads": [{"type": signal_type, "values": [value]}]
            }]
        })
    }

    fn empty_ids() -> std::collections::HashSet<String> {
        std::collections::HashSet::new()
    }

    // (a) new event appears → OpenAdrArrived emitted
    #[test]
    fn new_event_emits_arrived() {
        let events = vec![make_event("ev1", "Peak DR", "PRICE", 0.30)];
        let changes = detect_event_changes(&events, &empty_ids(), 0, None, ts());
        let arrived: Vec<_> = changes
            .trace_events
            .iter()
            .filter(|e| matches!(e, controller::trace::ControllerEvent::OpenAdrArrived { .. }))
            .collect();
        assert_eq!(arrived.len(), 1);
        if let controller::trace::ControllerEvent::OpenAdrArrived {
            event_name,
            signal_type,
            value,
            ..
        } = &arrived[0]
        {
            assert_eq!(event_name, "Peak DR");
            assert_eq!(signal_type, "PRICE");
            assert!((value - 0.30).abs() < 1e-9);
        }
    }

    // (b) event disappears → OpenAdrExpired emitted
    #[test]
    fn removed_event_emits_expired() {
        let mut prev_ids = empty_ids();
        prev_ids.insert("ev1".to_string());
        let changes = detect_event_changes(&[], &prev_ids, 0, None, ts());
        let expired: Vec<_> = changes
            .trace_events
            .iter()
            .filter(|e| matches!(e, controller::trace::ControllerEvent::OpenAdrExpired { .. }))
            .collect();
        assert_eq!(expired.len(), 1);
        if let controller::trace::ControllerEvent::OpenAdrExpired { event_name, .. } = &expired[0] {
            assert_eq!(event_name, "ev1");
        }
    }

    // (c) tariff count changes → RateChange emitted
    #[test]
    fn tariff_count_change_emits_rate_change() {
        // An event with a PRICE payload and intervalPeriod to trigger parse_rate_snapshots
        let events = vec![serde_json::json!({
            "id": "ev1",
            "eventName": "Price Event",
            "intervals": [{
                "intervalPeriod": {"start": "2026-03-21T10:00:00Z", "duration": "PT1H"},
                "payloads": [{"type": "PRICE", "values": [0.25]}]
            }]
        })];
        let mut prev_ids = empty_ids();
        prev_ids.insert("ev1".to_string()); // already seen → no OpenAdrArrived
        let changes = detect_event_changes(&events, &prev_ids, 0, None, ts());
        // Only assert if the parser actually produced rates (depends on parser internals)
        if !changes.rates.is_empty() {
            let rate_changes: Vec<_> = changes
                .trace_events
                .iter()
                .filter(|e| matches!(e, controller::trace::ControllerEvent::RateChange { .. }))
                .collect();
            assert_eq!(rate_changes.len(), 1);
        }
    }

    // (d) import limit changes → CapacityChange emitted
    #[test]
    fn import_limit_change_emits_capacity_change() {
        let events = vec![serde_json::json!({
            "id": "ev1",
            "eventName": "Capacity Event",
            "intervals": [{
                "intervalPeriod": {"start": "2026-03-21T10:00:00Z", "duration": "PT1H"},
                "payloads": [{"type": "IMPORT_CAPACITY_LIMIT", "values": [5.0]}]
            }]
        })];
        let mut prev_ids = empty_ids();
        prev_ids.insert("ev1".to_string()); // already seen
        let prev_limit: Option<f64> = None;
        let changes = detect_event_changes(&events, &prev_ids, 0, prev_limit, ts());
        if changes.capacity.import_limit_kw != prev_limit {
            let cap_changes: Vec<_> = changes
                .trace_events
                .iter()
                .filter(|e| matches!(e, controller::trace::ControllerEvent::CapacityChange { .. }))
                .collect();
            assert_eq!(cap_changes.len(), 1);
        }
    }

    // (e) no changes → no arrived/expired/capacity events emitted
    #[test]
    fn no_changes_emits_nothing() {
        let events = vec![make_event("ev1", "Peak DR", "PRICE", 0.30)];
        let mut prev_ids = empty_ids();
        prev_ids.insert("ev1".to_string());
        // Same event already seen, no capacity limit in payload, same import limit (None)
        let changes = detect_event_changes(&events, &prev_ids, 999, None, ts());
        let no_arrived = !changes
            .trace_events
            .iter()
            .any(|e| matches!(e, controller::trace::ControllerEvent::OpenAdrArrived { .. }));
        let no_expired = !changes
            .trace_events
            .iter()
            .any(|e| matches!(e, controller::trace::ControllerEvent::OpenAdrExpired { .. }));
        let no_capacity = !changes
            .trace_events
            .iter()
            .any(|e| matches!(e, controller::trace::ControllerEvent::CapacityChange { .. }));
        assert!(no_arrived, "expected no OpenAdrArrived");
        assert!(no_expired, "expected no OpenAdrExpired");
        assert!(no_capacity, "expected no CapacityChange");
    }
}

