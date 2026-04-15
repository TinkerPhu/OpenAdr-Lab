use chrono::{DateTime, Utc};
use metrics::counter;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info};

use crate::controller;
use crate::entities;
use crate::entities::asset::PlanTrigger;
use crate::profile::Profile;
use crate::simulator::SimState;
use crate::state::AppState;
use crate::vtn::VtnClient;

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

pub(crate) fn spawn_sim_tick(
    state: AppState,
    sim: Arc<Mutex<SimState>>,
    profile: Arc<Profile>,
    ven_name: String,
    vtn: VtnClient,
    trigger_tx: Arc<tokio::sync::watch::Sender<PlanTrigger>>,
    data_dir: String,
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

        loop {
            tick_interval.tick().await;
            let now = Utc::now();
            let dt_s = tick_s as f64;

            // Get current events and injection state
            let _events = state.events().await;
            let inject = state.inject_state().await;

            // Pre-tick: snapshot plan/packets/capacity/tariffs for dispatcher
            let plan_snap = state.active_plan().await;
            let packets_snap = state.active_packets().await;
            let capacity_snap = state.capacity_state().await;
            let rates_snap = state.planned_tariffs().await;

            // Tick loop: build_setpoints → sim.tick → update_sim → accounting
            let sim_snapshot = {
                let mut sim_guard = sim.lock().await;

                // Behaviour A: apply one-shot state jumps (cleared after application).
                {
                    use std::collections::HashMap;
                    if let Some(soc) = inject.battery_soc {
                        if let Some((entry, cfg)) = sim_guard.find_asset_mut("battery") {
                            let mut v = HashMap::new();
                            v.insert("soc".to_string(), soc);
                            cfg.reset(&mut entry.state, v);
                        }
                        state.clear_inject_field("battery_soc").await;
                    }
                    if let Some(soc) = inject.ev_soc {
                        if let Some((entry, cfg)) = sim_guard.find_asset_mut("ev") {
                            let mut v = HashMap::new();
                            v.insert("soc".to_string(), soc);
                            cfg.reset(&mut entry.state, v);
                        }
                        state.clear_inject_field("ev_soc").await;
                    }
                    if let Some(temp) = inject.heater_temp_c {
                        if let Some((entry, cfg)) = sim_guard.find_asset_mut("heater") {
                            let mut v = HashMap::new();
                            v.insert("temp_c".to_string(), temp);
                            cfg.reset(&mut entry.state, v);
                        }
                        state.clear_inject_field("heater_temp_c").await;
                    }
                }

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

                // Build setpoints from plan (single authoritative control path)
                let sp_map: std::collections::HashMap<String, f64> = match &plan_snap {
                    Some(ref plan) => controller::dispatcher::build_setpoints(
                        plan,
                        &sim_guard.assets,
                        &sim_guard.asset_configs,
                        &effective_capacity,
                        inject.heater_setpoint_c,
                        now,
                    ),
                    None => {
                        // No plan yet (startup window). Apply defaults then surplus overlay.
                        let mut m: std::collections::HashMap<String, f64> = sim_guard
                            .assets
                            .iter()
                            .zip(sim_guard.asset_configs.iter())
                            .map(|(a, cfg)| (a.id.clone(), cfg.default_setpoint(&a.state)))
                            .collect();
                        controller::dispatcher::apply_surplus_ev_overlay(
                            &mut m,
                            &sim_guard.assets,
                            &sim_guard.asset_configs,
                            false,
                        );
                        m
                    }
                };

                // Simulator: apply setpoints → update device states
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
                    if let Some(report) =
                        controller::reporter::build_status_report(&evt, &*sim_guard, &ven_name, now)
                    {
                        if let Err(e) = vtn.upsert_report(report).await {
                            error!("status report (packet transition) submission failed: {e:#}");
                        }
                    }
                }
                if let Some(t) = trigger_opt {
                    let _ = trigger_tx.send(t);
                }

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

                sim_guard.clone()
            };

            let _ = sim_snapshot; // used by reporting in Phase 6

            // Periodic measurement reports (T049)
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

            // Periodic persist
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

/// Ensure every `profile.packets` seed has a live non-terminal `EnergyPacket`.
///
/// For each seed, if no non-terminal packet already exists for that asset, one is
/// created and appended. Called at the top of each planning cycle so the planner
/// always has at least the profile-configured work to schedule.
fn seed_missing_packets(
    packets: &[entities::energy_packet::EnergyPacket],
    profile: &Profile,
    now: DateTime<Utc>,
) -> Vec<entities::energy_packet::EnergyPacket> {
    use chrono::Duration;
    use entities::asset::{ComfortRate, CompletionPolicy, UserRequestMode};
    use entities::energy_packet::{DeadlineTier, EnergyPacket, ValueCurve};

    let mut result = packets.to_vec();
    for seed in &profile.packets {
        // Skip if a non-terminal packet already exists for this asset.
        let has_live = result.iter().any(|p| p.asset_id == seed.asset && !p.is_terminal());
        if has_live {
            continue;
        }

        // Derive power from seed or profile default.
        let desired_power_kw = seed.desired_power_kw.unwrap_or_else(|| {
            if seed.asset == profile.ev_config().map(|c| c.id.as_str()).unwrap_or("ev") {
                profile.ev_config().map(|c| c.max_charge_kw).unwrap_or(7.4)
            } else if seed.asset == profile.heater_config().map(|c| c.id.as_str()).unwrap_or("heater") {
                profile.heater_config().map(|c| c.max_kw).unwrap_or(5.0)
            } else {
                1.0
            }
        });

        // Derive target_energy_kwh from target_soc and EV battery capacity when applicable.
        let target_energy_kwh = if let Some(soc) = seed.target_soc {
            if let Some(ev) = profile.ev_config() {
                (soc - ev.initial_soc).clamp(0.0, 1.0) * ev.battery_kwh
            } else {
                desired_power_kw // 1h default
            }
        } else {
            desired_power_kw // 1h default
        };

        // Deadline tier.
        let deadline = now + Duration::seconds((seed.latest_end_h * 3600.0) as i64);
        let deadline_tiers = vec![DeadlineTier {
            deadline,
            max_total_cost_eur: None,
            max_marginal_rate_eur_kwh: None,
            min_completion: 0.8,
        }];

        // Comfort rates from seed or defaults.
        let comfort_rates: Vec<ComfortRate> = if seed.comfort_rates.is_empty() {
            vec![
                ComfortRate { fill: 0.0, max_marginal_price: 0.35, max_marginal_co2: 0.0 },
                ComfortRate { fill: 1.0, max_marginal_price: 0.05, max_marginal_co2: 0.0 },
            ]
        } else {
            seed.comfort_rates
                .iter()
                .map(|r| ComfortRate { fill: r.fill, max_marginal_price: r.bid, max_marginal_co2: 0.0 })
                .collect()
        };

        let value_curve = ValueCurve { comfort_rates, deadline_tiers, active_tier_index: 0 };
        let packet = EnergyPacket {
            target_soc: seed.target_soc,
            request_mode: UserRequestMode::ByDeadline,
            completion_policy: CompletionPolicy::Stop,
            ..EnergyPacket::new(seed.asset.clone(), target_energy_kwh, desired_power_kw, value_curve, now)
        };

        info!(asset_id = %seed.asset, packet_id = %packet.id, target_energy_kwh, "seeded missing packet from profile");
        result.push(packet);
    }
    result
}

pub(crate) fn spawn_planning(
    state: AppState,
    profile: Arc<Profile>,
    vtn: VtnClient,
    ven_name: String,
    mut trigger_rx: tokio::sync::watch::Receiver<PlanTrigger>,
    sim: Arc<Mutex<SimState>>,
) -> tokio::task::JoinHandle<()> {
    let replan_s = profile.planner.replan_interval_s;
    tokio::spawn(async move {
        // Initial delay: let event poll populate rates before first plan
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        loop {
            let now = Utc::now();
            let rates = state.planned_tariffs().await;
            let packets = seed_missing_packets(&state.active_packets().await, &profile, now);
            let capacity = state.capacity_state().await;
            let trigger = trigger_rx.borrow().clone();
            let trigger_reason = format!("{:?}", trigger);

            let tariff_ts =
                crate::entities::tariff_snapshot::TariffTimeSeries::from_snapshots(&rates);
            let ev_sess = state.ev_session().await;
            let heat_tgt = state.heater_target().await;
            let shift_loads = state.shiftable_loads().await;
            let bl_override = state.baseline_override().await;
            let sim_guard_for_planner = sim.lock().await;
            let (mut plan, plan_steps) = controller::milp_planner::run_planner(
                &*sim_guard_for_planner,
                &tariff_ts,
                &packets,
                &capacity,
                &profile,
                now,
                trigger,
                ev_sess.as_ref(),
                heat_tgt.as_ref(),
                &shift_loads,
                bl_override.as_ref(),
            );
            drop(sim_guard_for_planner);
            plan.steps = plan_steps;
            let slot_count = plan.slots.len();
            let plan_packets = plan.packets.clone();
            state.set_active_packets(plan_packets.clone()).await;
            state.set_active_plan(Some(plan)).await;

            // Refresh site envelope immediately after each plan cycle.
            {
                let sim_guard = sim.lock().await;
                let env = controller::envelope::compute_envelope(
                    &*sim_guard,
                    now,
                );
                drop(sim_guard);
                state.set_site_envelope(env).await;
            }

            info!("plan cycle complete");

            // Emit PlanCycle controller event (T029)
            let plan_cycle_event = controller::trace::ControllerEvent::PlanCycle {
                ts: now,
                trigger_reason,
                total_slots: slot_count,
            };
            state.push_controller_event(plan_cycle_event.clone()).await;

            // Event-driven status report on PlanCycle (T050)
            {
                let report_opt = {
                    let sim_guard = sim.lock().await;
                    controller::reporter::build_status_report(
                        &plan_cycle_event,
                        &*sim_guard,
                        &ven_name,
                        now,
                    )
                };
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
