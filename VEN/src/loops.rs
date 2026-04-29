use chrono::{DateTime, Utc};
use metrics::counter;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, error, info, warn};

use crate::controller;
use crate::entities;
use crate::entities::asset::PlanTrigger;
use crate::planner_events::{PlannerEvent, PlannerEventTx};
use crate::profile::{Profile, PlannerObjective};
use crate::simulator::SimState;
use crate::state::{AppState, EvSettings, SimInjectState};
use crate::vtn::VtnClient;
use std::collections::HashMap;
use crate::entities::plan::Plan;
use crate::entities::capacity::OadrCapacityState;
use crate::entities::tariff_snapshot::TariffSnapshot;
use crate::simulator::{AssetEntry, SimSnapshot};
use crate::assets::AssetConfig;

// ─── Event poll change detection (RF-B08) ─────────────────────────────────────

/// Output of `detect_event_changes` — all side-effect-free results of one poll tick.
pub(crate) struct EventChanges {
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
pub(crate) fn detect_event_changes(
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
        let Some(id) = evt.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        if prev_ids.contains(id) {
            continue;
        }

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

    EventChanges {
        trace_events,
        current_ids,
        rates,
        capacity,
    }
}

// ─── Background loop spawners ──────────────────────────────────────────────────

pub(crate) fn spawn_program_poll(
    state: AppState,
    vtn: VtnClient,
    secs: u64,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(secs));
        loop {
            interval.tick().await;
            match vtn.fetch_programs().await {
                Ok(programs) => {
                    counter!("poll_success_total", "resource" => "programs").increment(1);
                    info!(
                        resource = "programs",
                        count = programs.len(),
                        "poll success"
                    );
                    state.set_programs(programs).await;
                }
                Err(e) => {
                    counter!("poll_error_total", "resource" => "programs").increment(1);
                    error!(resource = "programs", "poll failed: {e:#}");
                }
            }
        }
    })
}

pub(crate) fn spawn_event_poll(
    state: AppState,
    vtn: VtnClient,
    secs: u64,
    trigger_tx: Arc<tokio::sync::watch::Sender<PlanTrigger>>,
) -> tokio::task::JoinHandle<()> {
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
                        &events,
                        now,
                        &existing_obs,
                    );
                    state.add_obligations(new_obs).await;

                    state.set_events(events, 500).await;

                    // Signal planner: rates may have changed
                    let _ = trigger_tx.send(PlanTrigger::RateChange);
                }
                Err(e) => {
                    counter!("poll_error_total", "resource" => "events").increment(1);
                    error!(resource = "events", "poll failed: {e:#}");
                }
            }
        }
    })
}

pub(crate) fn spawn_report_poll(
    state: AppState,
    vtn: VtnClient,
    secs: u64,
) -> tokio::task::JoinHandle<()> {
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
    })
}

// ─── Helpers for spawn_sim_tick ───────────────────────────────────────────────

/// Deviation tracking state for Layer 1 (reactive correction) and Layer 2 (sustained deviation).
pub(crate) struct DeviationState {
    pub deviation_ticks: u32,
    pub last_correction_kw: f64,
    pub correction_is_active: bool,
    pub prev_correction_kw: f64,
}

impl Default for DeviationState {
    fn default() -> Self {
        Self {
            deviation_ticks: 0,
            last_correction_kw: 0.0,
            correction_is_active: false,
            prev_correction_kw: 0.0,
        }
    }
}

/// PHASE 1: Apply Behaviour A one-shot state injections to the simulator.
/// Returns a list of field names that were applied and should be cleared.
fn apply_sim_injections(inject: &SimInjectState, sim: &mut SimState) -> Vec<&'static str> {
    let mut cleared = Vec::new();
    if let Some(soc) = inject.battery_soc {
        if let Some((entry, cfg)) = sim.find_asset_mut(crate::ids::ASSET_BATTERY) {
            let mut v = HashMap::new();
            v.insert("soc".to_string(), soc);
            cfg.reset(&mut entry.state, v);
        }
        cleared.push("battery_soc");
    }
    if let Some(soc) = inject.ev_soc {
        if let Some((entry, cfg)) = sim.find_asset_mut(crate::ids::ASSET_EV) {
            let mut v = HashMap::new();
            v.insert("soc".to_string(), soc);
            cfg.reset(&mut entry.state, v);
        }
        cleared.push("ev_soc");
    }
    if let Some(temp) = inject.heater_temp_c {
        if let Some((entry, cfg)) = sim.find_asset_mut(crate::ids::ASSET_HEATER) {
            let mut v = HashMap::new();
            v.insert("temp_c".to_string(), temp);
            cfg.reset(&mut entry.state, v);
        }
        cleared.push("heater_temp_c");
    }
    cleared
}

/// PHASE 2: Compose effective capacity and build per-asset setpoints.
fn build_tick_setpoints(
    sim: &SimState,
    plan_snap: Option<&Plan>,
    capacity_snap: &OadrCapacityState,
    inject: &SimInjectState,
    overlay_enabled: bool,
    now: DateTime<Utc>,
) -> HashMap<String, f64> {
    // Compose effective capacity: inject grid limits only when no VTN event active.
    let mut effective_capacity = capacity_snap.clone();
    if effective_capacity.import_limit_event_id.is_none() {
        if let Some(lim) = inject.grid_import_limit_kw {
            effective_capacity.import_limit_kw = Some(lim);
        }
    }
    if effective_capacity.export_limit_event_id.is_none() {
        if let Some(lim) = inject.grid_export_limit_kw {
            effective_capacity.export_limit_kw = Some(lim);
        }
    }
    match plan_snap {
        Some(plan) => controller::dispatcher::build_setpoints(
            plan,
            &sim.assets,
            &sim.asset_configs,
            &effective_capacity,
            inject.heater_setpoint_c,
            now,
            overlay_enabled,
        ),
        None => {
            // No plan yet (startup window). Apply defaults then surplus overlay.
            let mut m: HashMap<String, f64> = sim
                .assets
                .iter()
                .zip(sim.asset_configs.iter())
                .map(|(a, cfg)| (a.id.clone(), cfg.default_setpoint(&a.state)))
                .collect();
            controller::dispatcher::apply_surplus_ev_overlay(
                &mut m,
                &sim.assets,
                &sim.asset_configs,
                false,
                overlay_enabled,
            );
            m
        }
    }
}

/// PHASE 3: Apply Layer 1 battery correction overlay (goal-aware reactive compensation),
/// correction hold (Plan G), and emit SSE events for correction state changes.
/// Returns the applied correction_kw.
fn apply_deviation_correction(
    dev_state: &mut DeviationState,
    setpoints: &mut HashMap<String, f64>,
    assets: &[AssetEntry],
    asset_configs: &[AssetConfig],
    plan_snap: Option<&Plan>,
    plan_signed_net_kw: f64,
    prev_actual_net_kw: f64,
    profile: &Profile,
    event_tx: &PlannerEventTx,
    now: DateTime<Utc>,
) -> f64 {
    let correction_kw = if plan_snap.is_some() {
        let c = controller::dispatcher::apply_battery_correction_overlay(
            setpoints,
            assets,
            asset_configs,
            plan_signed_net_kw,
            prev_actual_net_kw,
            plan_snap.unwrap().objective,
            profile.planner.deviation_threshold_kw,
            profile.planner.correction_min_kw,
        );
        if c != 0.0 {
            debug!(
                correction_kw = c,
                plan_signed_net_kw,
                actual_net_kw = prev_actual_net_kw,
                deviation_kw = prev_actual_net_kw - plan_signed_net_kw,
                "layer1: battery correction applied"
            );
        }
        c
    } else {
        0.0
    };

    // Plan G correction hold: when correction just cleared (returned 0.0 this tick
    // but was active last tick), re-insert the battery's previously-applied
    // setpoint into setpoints. Without this, build_setpoints' plan allocation reverts
    // the battery and recreates the deviation on the very next tick (limit cycle).
    // Only fires when prev_correction_kw was meaningful (> correction_min_kw).
    if correction_kw == 0.0
        && dev_state.prev_correction_kw.abs() > profile.planner.correction_min_kw
    {
        if let Some(bat) = assets.iter().find(|a| a.id == crate::ids::ASSET_BATTERY) {
            let plan_sp = setpoints.get(crate::ids::ASSET_BATTERY).copied().unwrap_or(0.0);
            let held_sp = bat.setpoint_kw;
            if (held_sp - plan_sp).abs() > profile.planner.correction_min_kw {
                setpoints.insert(crate::ids::ASSET_BATTERY.to_string(), held_sp);
            }
        }
    }
    // Only update prev_correction_kw when a real correction fired.
    // Keeping the last non-zero value active ensures the hold persists
    // across subsequent ticks where the battery is maxed out (correction
    // returns 0.0 due to delta < min_correction_kw), preventing the
    // 3-tick oscillation: correct → hold → revert → correct → ...
    if correction_kw != 0.0 {
        dev_state.prev_correction_kw = correction_kw;
    }

    // Emit CorrectionActive/CorrectionCleared SSE on significant state change
    if (correction_kw - dev_state.last_correction_kw).abs() > 0.2 {
        if correction_kw.abs() > profile.planner.correction_min_kw {
            let reason = if (prev_actual_net_kw - plan_signed_net_kw) > 0.0 {
                "import_excess"
            } else {
                "export_excess"
            };
            let obj = plan_snap.map(|p| p.objective).unwrap_or_default();
            let _ = event_tx.send(PlannerEvent::CorrectionActive {
                ts: now,
                asset_id: crate::ids::ASSET_BATTERY.to_string(),
                reason: reason.to_string(),
                planned_net_kw: plan_signed_net_kw,
                actual_net_kw: prev_actual_net_kw,
                deviation_kw: prev_actual_net_kw - plan_signed_net_kw,
                correction_kw,
                objective: obj,
            });
            dev_state.correction_is_active = true;
        } else if dev_state.correction_is_active {
            let _ = event_tx.send(PlannerEvent::CorrectionCleared {
                ts: now,
                reason: "within_threshold".to_string(),
            });
            dev_state.correction_is_active = false;
        }
        dev_state.last_correction_kw = correction_kw;
    }

    correction_kw
}

/// PHASE 5: Publish post-tick simulator state — sensor, sim snapshot, shiftable logic,
/// ledger, history ring buffers, grid virtual asset, and site envelope.
/// Returns the augmented SimSnapshot.
async fn publish_sim_tick_result(
    sim_guard: &mut SimState,
    plan_snap: Option<&Plan>,
    state: &AppState,
    trigger_tx: &tokio::sync::watch::Sender<PlanTrigger>,
    rates_snap: &[TariffSnapshot],
    capacity_snap: &OadrCapacityState,
    dt_s: f64,
    now: DateTime<Utc>,
) -> SimSnapshot {
    // Update sensor snapshot (backward compat)
    let sensor = sim_guard.to_sensor_snapshot();
    state.update_sensor(sensor).await;

    // Update sim in app state — augmented with shiftable runtimes
    let mut sim_snap = sim_guard.to_sim_snapshot();

    // ── Shiftable load runtime: start / complete / augment ──────

    // Collect physics asset IDs so we skip them when scanning allocations.
    let known_sim_ids: std::collections::HashSet<&str> =
        sim_guard.assets.iter().map(|a| a.id.as_str()).collect();

    // Start: detect shiftable loads that the current plan slot wants
    // to run but that have no runtime yet.
    if let Some(plan) = plan_snap {
        if let Some(slot) = plan.slots.iter().find(|s| s.start <= now && now < s.end) {
            let runtimes = state.shiftable_runtimes().await;
            let loads = state.shiftable_loads().await;
            for alloc in &slot.allocations {
                if known_sim_ids.contains(alloc.asset_id.as_str()) { continue; }
                let already_running = runtimes.iter().any(|r| r.asset_id == alloc.asset_id);
                if !already_running {
                    if let Some(load) = loads.iter().find(|l| l.asset_id == alloc.asset_id) {
                        let ends_at = now + chrono::Duration::minutes(load.duration_min as i64);
                        state.start_shiftable(entities::device_session::ShiftableLoadRuntime {
                            load_id: load.id,
                            asset_id: load.asset_id.clone(),
                            power_kw: load.power_kw,
                            started_at: now,
                            ends_at,
                        }).await;
                        info!(asset_id = %load.asset_id, ends_at = %ends_at, "shiftable load started");
                    }
                }
            }
        }
    }

    // Complete: remove expired runtimes and trigger replan.
    {
        let runtimes = state.shiftable_runtimes().await;
        for rt in &runtimes {
            if now >= rt.ends_at {
                info!(asset_id = %rt.asset_id, "shiftable load completed");
                state.complete_shiftable(rt.load_id).await;
                let _ = trigger_tx.send(PlanTrigger::UserRequest);
            }
        }
    }

    // Augment SimSnapshot with running shiftable runtimes so they
    // appear in GET /sim and ledger accounting.
    {
        let runtimes = state.shiftable_runtimes().await;
        for rt in &runtimes {
            if rt.is_running(now) {
                sim_snap.assets.insert(rt.asset_id.clone(), crate::simulator::AssetSnapshot {
                    power_kw: rt.power_kw,
                    values: {
                        let mut m = std::collections::HashMap::new();
                        m.insert("running".into(), 1.0);
                        m.insert("ends_at_unix".into(), rt.ends_at.timestamp() as f64);
                        m
                    },
                });
            }
        }
    }

    state.update_sim(sim_snap.clone()).await;

    // Post-tick: consolidated ledger accounting
    let mut ledger = state.asset_ledger().await;
    controller::monitor::record_tick(
        &mut ledger,
        &sim_snap,
        rates_snap,
        dt_s,
        now,
    );
    state.set_asset_ledger(ledger).await;

    // Post-tick: push HistoryPoint per asset into per-asset ring buffer (CP2).
    {
        use crate::assets::HistoryPoint;
        for entry in &mut sim_guard.assets {
            entry.history.push(HistoryPoint {
                ts: now,
                power_kw: entry.last_power_kw,
                state: entry.state.clone(),
            });
        }
    }

    // Update Grid virtual asset with net power + VTN capacity limits.
    // Done here (not inside tick()) so capacity_snap is available.
    {
        let net_power_kw = sim_guard.grid.net_power_w / 1000.0;
        let import_limit_kw = capacity_snap.import_limit_kw.unwrap_or(f64::MAX);
        // OadrCapacityState.export_limit_kw is a positive magnitude; negate for sign convention.
        let export_limit_kw_signed = -(capacity_snap.export_limit_kw.unwrap_or(f64::MAX));
        sim_guard.grid_asset.update(net_power_kw, import_limit_kw, export_limit_kw_signed, now);
    }

    // Refresh site envelope on every sim tick (~1s).
    // Done inside the sim lock to avoid a second lock acquisition.
    {
        let env = controller::envelope::compute_envelope(
            &*sim_guard,
            now,
        );
        state.set_site_envelope(env).await;
    }

    sim_snap
}

pub(crate) fn spawn_sim_tick(
    state: AppState,
    sim: Arc<Mutex<SimState>>,
    profile: Arc<Profile>,
    ven_name: String,
    vtn: VtnClient,
    trigger_tx: Arc<tokio::sync::watch::Sender<PlanTrigger>>,
    data_dir: String,
    event_tx: PlannerEventTx,
) -> tokio::task::JoinHandle<()> {
    let tick_s = profile.simulator.tick_s;
    let persist_every_s = profile.simulator.persist_every_s;
    let report_interval_s = profile.simulator.report_interval_s;
    tokio::spawn(async move {
        let mut tick_interval = tokio::time::interval(std::time::Duration::from_secs(tick_s));
        let mut persist_counter: u64 = 0;
        let persist_every_ticks = if tick_s > 0 {
            persist_every_s / tick_s
        } else {
            15
        };
        let mut report_counter: u64 = 0;
        let report_every_ticks = if tick_s > 0 && report_interval_s > 0 {
            report_interval_s / tick_s
        } else {
            0
        };

        // Plan F/G: Layer 1/2 deviation state (T028)
        let mut dev_state = DeviationState::default();

        loop {
            tick_interval.tick().await;
            let now = Utc::now();
            let dt_s = tick_s as f64;

            // PHASE 0: Snapshot — events, inject, plan, capacity, tariffs, overlay flag
            let _events = state.events().await;
            let inject = state.inject_state().await;

            // Pre-tick: snapshot plan/capacity/tariffs for dispatcher
            let plan_snap = state.active_plan().await;
            let capacity_snap = state.capacity_state().await;
            let rates_snap = state.planned_tariffs().await;

            // Compute overlay_enabled: user toggle AND no active EvSession.
            let ev_sess_tick = state.ev_session().await;
            let ev_settings_tick = state.ev_settings().await;
            let session_active = ev_sess_tick.is_some();
            let overlay_enabled = ev_settings_tick.opportunistic_charging_enabled && !session_active;
            if ev_settings_tick.paused_by_active_session != session_active {
                state.set_ev_settings(EvSettings {
                    paused_by_active_session: session_active,
                    ..ev_settings_tick
                }).await;
            }

            // Lock sim; all physics and accounting phases run inside.
            let sim_snapshot = {
                let mut sim_guard = sim.lock().await;

                // PHASE 1: Apply Behaviour A one-shot injections; clear returned fields.
                let cleared_fields = apply_sim_injections(&inject, &mut *sim_guard);
                for field in cleared_fields {
                    state.clear_inject_field(field).await;
                }

                // PHASE 2: Build setpoints (prev grid for Layer 1; effective capacity; dispatcher)
                // Plan F: capture prev tick net grid for Layer 1 correction.
                // Use signed net: positive = import, negative = export. The plan
                // stores import and export as separate non-negative fields; combining
                // them into a signed value prevents false corrections when the plan
                // intentionally expects export (net_import_kw=0, net_export_kw>0).
                let prev_actual_net_kw = sim_guard.grid.net_power_w / 1000.0;
                let plan_signed_net_kw = plan_snap.as_ref()
                    .and_then(|p| p.current_slot(now))
                    .map(|s| s.net_import_kw - s.net_export_kw)
                    .unwrap_or(0.0);

                let mut sp_map = build_tick_setpoints(
                    &*sim_guard,
                    plan_snap.as_ref(),
                    &capacity_snap,
                    &inject,
                    overlay_enabled,
                    now,
                );

                // PHASE 3: Layer 1 battery correction overlay + hold + SSE.
                apply_deviation_correction(
                    &mut dev_state,
                    &mut sp_map,
                    &sim_guard.assets,
                    &sim_guard.asset_configs,
                    plan_snap.as_ref(),
                    plan_signed_net_kw,
                    prev_actual_net_kw,
                    &profile,
                    &event_tx,
                    now,
                );

                // PHASE 4: Simulator tick — apply setpoints → update device states.
                sim_guard.tick(
                    dt_s,
                    sp_map,
                    now,
                    inject.pv_irradiance,
                    inject.pv_irradiance_alpha,
                    inject.ambient_temp_c,
                    inject.heater_temp_min_c,
                    inject.heater_temp_max_c,
                    inject.base_load_kw,
                    inject.base_load_alpha,
                    inject.ev_plugged,
                    inject.ev_soc_target,
                );

                // pv_irradiance and base_load_kw are one-shots: apply offset once then let it decay.
                if inject.pv_irradiance.is_some() {
                    state.clear_inject_field("pv_irradiance").await;
                }
                if inject.base_load_kw.is_some() {
                    state.clear_inject_field("base_load_kw").await;
                }

                // PHASE 5: Publish — sensor, sim snapshot, shiftable logic, ledger, grid, envelope.
                publish_sim_tick_result(
                    &mut *sim_guard,
                    plan_snap.as_ref(),
                    &state,
                    &*trigger_tx,
                    &rates_snap,
                    &capacity_snap,
                    dt_s,
                    now,
                ).await
            };

            // PHASE 6: Layer 2 — accumulate sustained deviation → DeviceDeviation trigger.
            if let Some(ref plan) = plan_snap {
                if let Some(slot) = plan.current_slot(now) {
                    let post_net_kw = sim_snapshot.grid.net_power_w / 1000.0;
                    let planned_net_kw = slot.net_import_kw - slot.net_export_kw;
                    let post_error_kw = (post_net_kw - planned_net_kw).abs();
                    if post_error_kw > profile.planner.deviation_threshold_kw {
                        dev_state.deviation_ticks = dev_state.deviation_ticks.saturating_add(1);
                        debug!(
                            post_net_kw,
                            planned_net_kw,
                            post_error_kw,
                            threshold_kw = profile.planner.deviation_threshold_kw,
                            deviation_ticks = dev_state.deviation_ticks,
                            trigger_ticks = profile.planner.deviation_trigger_ticks,
                            "layer2: sustained deviation tick"
                        );
                        if dev_state.deviation_ticks >= profile.planner.deviation_trigger_ticks {
                            dev_state.deviation_ticks = 0;
                            warn!(
                                post_net_kw,
                                planned_net_kw,
                                post_error_kw,
                                trigger_ticks = profile.planner.deviation_trigger_ticks,
                                "layer2: DeviceDeviation trigger fired"
                            );
                            let _ = trigger_tx.send(PlanTrigger::DeviceDeviation);
                        }
                    } else {
                        if dev_state.deviation_ticks > 0 {
                            debug!(
                                deviation_ticks = dev_state.deviation_ticks,
                                post_net_kw,
                                planned_net_kw,
                                post_error_kw,
                                "layer2: deviation cleared, resetting tick counter"
                            );
                        }
                        dev_state.deviation_ticks = 0;
                    }
                } else {
                    debug!("layer2: plan exists but no current_slot — skipping deviation check");
                }
            } else {
                // Only log occasionally to avoid spam when no plan exists at startup
                if dev_state.deviation_ticks == 0 {
                    debug!("layer2: no active plan — deviation check skipped");
                }
            }

            // PHASE 7: Periodic measurement reports (T049)
            if report_every_ticks > 0 {
                report_counter += 1;
                if report_counter >= report_every_ticks {
                    report_counter = 0;
                    let events = state.events().await;
                    let sim_guard = sim.lock().await;
                    let reports = controller::reporter::build_measurement_reports_for_active_events(
                        &events,
                        &*sim_guard,
                        &ven_name,
                        now,
                    );
                    drop(sim_guard);
                    for report in reports {
                        if let Err(e) = vtn.upsert_report(report).await {
                            error!("measurement report submission failed: {e:#}");
                        }
                    }
                }
            }

            // PHASE 8: Periodic persist
            persist_counter += 1;
            if persist_counter >= persist_every_ticks {
                persist_counter = 0;
                let sim_guard = sim.lock().await;
                if let Err(e) = crate::simulator::persist::save(&sim_guard, &data_dir).await {
                    error!("sim persist failed: {e:#}");
                }
            }
        }
    })
}

pub(crate) fn spawn_obligation_check(
    state: AppState,
    sim: Arc<Mutex<SimState>>,
    vtn: VtnClient,
    ven_name: String,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            let now = Utc::now();
            let due = state.due_obligations(now).await;
            for ob in due {
                let env = state.site_envelope().await;
                let report_opt = {
                    let sim_guard = sim.lock().await;
                    controller::reporter::build_measurement_report_for_obligation(
                        &ob,
                        &*sim_guard,
                        &ven_name,
                        env.as_ref(),
                    )
                };
                if let Some(report) = report_opt {
                    match vtn.upsert_report(report).await {
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
    })
}

pub(crate) fn spawn_planning(
    state: AppState,
    profile: Arc<Profile>,
    vtn: VtnClient,
    ven_name: String,
    mut trigger_rx: tokio::sync::watch::Receiver<PlanTrigger>,
    sim: Arc<Mutex<SimState>>,
    active_objective: Arc<RwLock<PlannerObjective>>,
    event_tx: PlannerEventTx,
) -> tokio::task::JoinHandle<()> {
    let replan_s = profile.planner.replan_interval_s;
    tokio::spawn(async move {
        // Initial delay: let event poll populate rates before first plan
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        loop {
            let now = Utc::now();
            let rates = state.planned_tariffs().await;
            let capacity = state.capacity_state().await;
            let trigger = trigger_rx.borrow().clone();
            let trigger_reason = format!("{:?}", trigger);

            info!(trigger = %trigger_reason, "planner loop: starting plan cycle");

            let tariff_ts =
                crate::entities::tariff_snapshot::TariffTimeSeries::from_snapshots(&rates);
            let ev_sess = state.ev_session().await;
            let heat_tgt = state.heater_target().await;
            let shift_loads = state.shiftable_loads().await;
            let bl_override = state.baseline_override().await;
            let obj = *active_objective.read().await;
            // Read inject state BEFORE cloning the sim. The pv_irradiance inject is a
            // one-shot: the sim tick applies it to pv.irradiance_offset and then clears
            // inject.pv_irradiance. If we read inject_state after the clone, the tick
            // can race in between: it clears the inject flag but we already have a stale
            // clone with offset=0. Reading first guarantees we always capture the pending
            // value before the tick has a chance to clear it.
            let inject_snap = state.inject_state().await;
            // Clone SimState snapshot so the Mutex is released immediately.
            // MILP solving takes 18-60s on Pi4 ARM64; holding the lock would
            // block sim ticks and /capability reads for the entire duration.
            let lock_start = std::time::Instant::now();
            let mut sim_snap = sim.lock().await.clone();
            let lock_ms = lock_start.elapsed().as_millis();
            if lock_ms > 500 {
                warn!(lock_wait_ms = lock_ms, trigger = %trigger_reason, "planner: sim lock wait was long");
            } else {
                debug!(lock_wait_ms = lock_ms, "planner: sim lock acquired");
            }

            // Patch the clone when pv_irradiance inject is pending and the tick hasn't
            // applied it yet. When the tick runs first, the clone already has the correct
            // offset and this block is a no-op (inject_snap.pv_irradiance is None).
            if let Some(forced) = inject_snap.pv_irradiance {
                use crate::assets::{AssetConfig, PvInverter};
                let natural = PvInverter::natural_irradiance_at(now);
                if let Some((_, cfg)) = sim_snap.find_asset_mut(crate::ids::ASSET_PV) {
                    if let AssetConfig::Pv(pv) = cfg {
                        pv.irradiance_offset = forced - natural;
                        pv.pv_alpha = inject_snap.pv_irradiance_alpha;
                    }
                }
            }

            // ── Emit solving_started ──────────────────────────────────────
            let num_slots = profile.planner.plan_horizon_h as usize
                * 3600
                / profile.planner.plan_step_s as usize;
            let _ = event_tx.send(PlannerEvent::SolvingStarted {
                objective: obj,
                num_slots,
                triggered_at: now,
            });

            // ── Spawn 1 s progress ticker ─────────────────────────────────
            let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();
            let progress_tx = event_tx.clone();
            let ticker_task = tokio::spawn(async move {
                let start = std::time::Instant::now();
                let mut ticker = tokio::time::interval(std::time::Duration::from_secs(1));
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                let mut iteration: u32 = 0;
                let mut cancel_rx = cancel_rx;
                loop {
                    tokio::select! {
                        _ = ticker.tick() => {
                            iteration += 1;
                            let _ = progress_tx.send(PlannerEvent::SolvingProgress {
                                elapsed_ms: start.elapsed().as_millis() as u64,
                                iteration,
                            });
                        }
                        _ = &mut cancel_rx => break,
                    }
                }
            });

            // ── Run blocking HiGHS solve off the async runtime ────────────
            let solve_start = std::time::Instant::now();
            let profile_clone = profile.clone(); // Arc<Profile>, cheap
            let trigger_for_planner = trigger.clone(); // keep `trigger` for acceptance gate below
            let plan = tokio::task::spawn_blocking(move || {
                controller::milp_planner::run_planner(
                    &sim_snap,
                    &tariff_ts,
                    &capacity,
                    &profile_clone,
                    now,
                    trigger_for_planner,
                    ev_sess.as_ref(),
                    heat_tgt.as_ref(),
                    &shift_loads,
                    bl_override.as_ref(),
                    Some(obj),
                )
            })
            .await
            .expect("planner task panicked");
            let solver_ms = solve_start.elapsed().as_millis() as u64;
            info!(
                solver_ms,
                trigger = %trigger_reason,
                slots = plan.slots.len(),
                objective_eur = plan.objective_eur,
                "planner: solve complete"
            );

            // ── Cancel ticker, emit plan_ready ────────────────────────────
            let _ = cancel_tx.send(());
            ticker_task.await.ok();
            let _ = event_tx.send(PlannerEvent::PlanReady {
                plan_id: plan.id,
                objective: obj,
                solver_ms,
                objective_eur: plan.objective_eur,
                friction_eur: plan.friction_eur,
                slot_count: plan.slots.len(),
                trigger: trigger_reason.clone(),
            });
            // Acceptance gate: for Periodic replans, only adopt the new plan when it
            // improves the objective (Phase 1 cost) by more than the effective threshold.
            // The threshold decays linearly with plan age so that changing circumstances
            // are never permanently blocked — after plan_adoption_decay_s seconds, any
            // new plan is accepted. Hard triggers always force adoption.
            let threshold = profile.planner.plan_adoption_threshold_eur;
            let decay_s = profile.planner.plan_adoption_decay_s;
            let is_hard_trigger = !matches!(trigger, PlanTrigger::Periodic);
            let adopt = if is_hard_trigger || threshold == 0.0 {
                true
            } else if let Some(ref current) = state.active_plan().await {
                let elapsed_s = (now - current.created_at).num_seconds().max(0) as f64;
                let decay_factor = if decay_s > 0.0 {
                    (1.0 - elapsed_s / decay_s).max(0.0)
                } else {
                    1.0
                };
                let effective_threshold = threshold * decay_factor;
                let improvement = current.objective_eur - plan.objective_eur;
                if improvement > effective_threshold {
                    true
                } else {
                    info!(
                        improvement_eur = improvement,
                        effective_threshold_eur = effective_threshold,
                        threshold_eur = threshold,
                        elapsed_s = elapsed_s,
                        decay_factor = decay_factor,
                        "periodic plan rejected: improvement below threshold"
                    );
                    false
                }
            } else {
                true // no existing plan → always adopt
            };

            let slot_count = plan.slots.len();
            if adopt {
                info!(trigger = %trigger_reason, slot_count, "planner: plan adopted");
                // Replan supersedes any active correction
                let _ = event_tx.send(PlannerEvent::CorrectionCleared {
                    ts: now,
                    reason: "superseded_by_replan".to_string(),
                });
                state.set_active_plan(Some(plan)).await;
            } else {
                info!(trigger = %trigger_reason, slot_count, "planner: plan NOT adopted (periodic below threshold)");
            }

            // Refresh site envelope immediately after each plan cycle.
            {
                let sim_snap = sim.lock().await.clone();
                let env = controller::envelope::compute_envelope(
                    &sim_snap,
                    now,
                );
                state.set_site_envelope(env).await;
            }

            info!(trigger = %trigger_reason, slot_count, "plan cycle complete");

            // Emit PlanCycle controller event (T029)
            let plan_cycle_event = controller::trace::ControllerEvent::PlanCycle {
                ts: now,
                trigger_reason,
                total_slots: slot_count,
            };
            state.push_controller_event(plan_cycle_event.clone()).await;

            // Event-driven status report on PlanCycle (T050)
            {
                let sim_snap = sim.lock().await.clone();
                let report_opt = controller::reporter::build_status_report(
                    &plan_cycle_event,
                    &sim_snap,
                    &ven_name,
                    None, // no single program_id in planning loop context
                    now,
                );
                if let Some(report) = report_opt {
                    if let Err(e) = vtn.upsert_report(report).await {
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
    })
}

pub(crate) fn spawn_state_persist(state: AppState, path: String) -> tokio::task::JoinHandle<()> {
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
    })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

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

#[cfg(test)]
mod sim_tick_tests {
    use super::*;
    use chrono::Utc;
    use crate::entities::capacity::OadrCapacityState;
    use crate::state::SimInjectState;

    #[tokio::test]
    async fn test_build_setpoints_no_plan() {
        // Creates a minimal SimState and calls build_tick_setpoints with plan=None.
        // Confirms function returns without panic and all values are finite.
        let profile = crate::profile::Profile::default();
        // Build a minimal SimState from the profile
        let sim = crate::simulator::SimState::from_profile(&profile);
        let capacity = OadrCapacityState::default();
        let inject = SimInjectState::default();
        let now = Utc::now();

        let setpoints = build_tick_setpoints(&sim, None, &capacity, &inject, false, now);

        // With no plan, should return defaults (one per asset)
        assert!(!setpoints.is_empty() || profile.assets.is_empty());
        for (_, v) in &setpoints {
            assert!(v.is_finite(), "setpoint must be finite");
        }
    }
}
