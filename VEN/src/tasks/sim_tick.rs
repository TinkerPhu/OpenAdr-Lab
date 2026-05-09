// Extracted from VEN/src/loops.rs — simulator tick (spawn_sim_tick)

use chrono::{DateTime, Utc};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, error, info, warn};

use crate::assets::AssetConfig;
use crate::controller;
use crate::entities;
use crate::entities::asset::PlanTrigger;
use crate::entities::capacity::OadrCapacityState;
use crate::entities::plan::Plan;
use crate::entities::plan::SiteFlexibilityEnvelope;
use crate::entities::tariff_snapshot::TariffSnapshot;
use crate::models::SensorSnapshot;
use crate::planner_events::{PlannerEvent, PlannerEventTx};
use crate::profile::{PlannerObjective, Profile};
use crate::simulator::SimState;
use crate::simulator::{AssetEntry, SimSnapshot};
use crate::state::{AppState, EvSettings, SimInjectState};
use crate::vtn::VtnClient;
use std::collections::HashMap;

use crate::controller::absorber::AbsorberState;

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

/// PHASE 5 (post-lock): Publish post-tick simulator state — sensor, sim snapshot, shiftable
/// logic, ledger, and site envelope. Called after the sim Mutex is released, so HTTP handlers
/// are never blocked by this async work.
/// Returns the augmented SimSnapshot.
async fn publish_sim_tick_result(
    sensor: SensorSnapshot,
    mut sim_snap: SimSnapshot,
    envelope: SiteFlexibilityEnvelope,
    plan_snap: Option<&Plan>,
    state: &AppState,
    trigger_tx: &tokio::sync::watch::Sender<PlanTrigger>,
    rates_snap: &[TariffSnapshot],
    dt_s: f64,
    now: DateTime<Utc>,
) -> SimSnapshot {
    // Update sensor snapshot (backward compat)
    state.update_sensor(sensor).await;

    // Update sim in app state — augmented with shiftable runtimes

    // ── Shiftable load runtime: start / complete / augment ──────

    // Start: detect shiftable loads that the current plan slot wants
    // to run but that have no runtime yet.
    if let Some(plan) = plan_snap {
        if let Some(slot) = plan.slots.iter().find(|s| s.start <= now && now < s.end) {
            let runtimes = state.shiftable_runtimes().await;
            let loads = state.shiftable_loads().await;
            for alloc in &slot.allocations {
                if sim_snap.assets.contains_key(alloc.asset_id.as_str()) {
                    continue;
                }
                let already_running = runtimes.iter().any(|r| r.asset_id == alloc.asset_id);
                if !already_running {
                    if let Some(load) = loads.iter().find(|l| l.asset_id == alloc.asset_id) {
                        let ends_at = now + chrono::Duration::minutes(load.duration_min as i64);
                        state
                            .start_shiftable(entities::device_session::ShiftableLoadRuntime {
                                load_id: load.id,
                                asset_id: load.asset_id.clone(),
                                power_kw: load.power_kw,
                                started_at: now,
                                ends_at,
                            })
                            .await;
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
                sim_snap.assets.insert(
                    rt.asset_id.clone(),
                    crate::simulator::AssetSnapshot {
                        power_kw: rt.power_kw,
                        values: {
                            let mut m = std::collections::HashMap::new();
                            m.insert("running".into(), 1.0);
                            m.insert("ends_at_unix".into(), rt.ends_at.timestamp() as f64);
                            m
                        },
                    },
                );
            }
        }
    }

    state.update_sim(sim_snap.clone()).await;

    // Post-tick: consolidated ledger accounting
    let mut ledger = state.asset_ledger().await;
    controller::monitor::record_tick(&mut ledger, &sim_snap, rates_snap, dt_s, now);
    state.set_asset_ledger(ledger).await;

    // Refresh site envelope (computed in-lock from final sim state).
    state.set_site_envelope(envelope).await;

    sim_snap
}

/// PHASE 5 in-lock tail: extract snapshots, push history, update grid asset, compute envelope.
/// Returns the 3-tuple needed for post-lock async state publishing.
fn finalize_tick_outputs(
    sim: &mut SimState,
    capacity_snap: &OadrCapacityState,
    now: DateTime<Utc>,
) -> (SensorSnapshot, SimSnapshot, SiteFlexibilityEnvelope) {
    let tick_sensor = sim.to_sensor_snapshot();
    let tick_sim_snap = sim.to_sim_snapshot();

    // Push HistoryPoint per asset into per-asset ring buffer (CP2).
    {
        use crate::assets::HistoryPoint;
        for entry in &mut sim.assets {
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
        let net_power_kw = sim.grid.net_power_w / 1000.0;
        let import_limit_kw = capacity_snap.import_limit_kw.unwrap_or(f64::MAX);
        // OadrCapacityState.export_limit_kw is a positive magnitude; negate for sign convention.
        let export_limit_kw_signed = -(capacity_snap.export_limit_kw.unwrap_or(f64::MAX));
        sim.grid_asset
            .update(net_power_kw, import_limit_kw, export_limit_kw_signed, now);
    }

    // Compute site envelope (pure math — reads sim, returns owned value).
    let tick_envelope = controller::envelope::compute_envelope(&*sim, now);

    (tick_sensor, tick_sim_snap, tick_envelope)
}

/// PHASE 6: Layer 2 — accumulate absorbed residual deviation → DeviceDeviation trigger.
/// Tier 2 escalates to MILP replanning when Tier 1 (absorber) cannot fully cover the grid deviation
/// for a sustained duration (deviation_trigger_ticks). Uses residual_kw (uncovered after absorption)
/// instead of raw post_net_kw to avoid triggering replans for transient deviations that the
/// absorber can handle.
fn accumulate_deviation(
    absorber_state: &mut AbsorberState,
    residual_kw: f64,
    profile: &Profile,
    trigger_tx: &tokio::sync::watch::Sender<PlanTrigger>,
    deviation_pending: &std::sync::atomic::AtomicBool,
    now: DateTime<Utc>,
) {
    // Residual exceeds dead-band: increment sustained deviation counter
    if residual_kw.abs() > profile.absorber.dead_band_kw {
        absorber_state.residual_ticks = absorber_state.residual_ticks.saturating_add(1);
        debug!(
            residual_kw,
            dead_band_kw = profile.absorber.dead_band_kw,
            residual_ticks = absorber_state.residual_ticks,
            trigger_ticks = profile.planner.deviation_trigger_ticks,
            "layer2: sustained residual deviation tick"
        );
        if absorber_state.residual_ticks >= profile.planner.deviation_trigger_ticks {
            absorber_state.residual_ticks = 0;
            warn!(
                residual_kw,
                dead_band_kw = profile.absorber.dead_band_kw,
                trigger_ticks = profile.planner.deviation_trigger_ticks,
                "layer2: DeviceDeviation trigger fired (absorber exhausted)"
            );
            let _ = trigger_tx.send(PlanTrigger::DeviceDeviation);
            deviation_pending.store(true, std::sync::atomic::Ordering::Release);
        }
    } else {
        if absorber_state.residual_ticks > 0 {
            debug!(
                residual_ticks = absorber_state.residual_ticks,
                residual_kw, "layer2: residual deviation cleared, resetting counter"
            );
        }
        absorber_state.residual_ticks = 0;
    }
}

/// PHASE 7: Periodic measurement reports.
async fn run_measurement_reports(
    state: &AppState,
    sim: &Arc<Mutex<SimState>>,
    vtn: &VtnClient,
    ven_name: &str,
    now: DateTime<Utc>,
) {
    let events = state.events().await;
    let sim_guard = sim.lock().await;
    let reports = controller::reporter::build_measurement_reports_for_active_events(
        &events,
        &*sim_guard,
        ven_name,
        now,
    );
    drop(sim_guard);
    for report in reports {
        if let Err(e) = vtn.upsert_report(report).await {
            error!("measurement report submission failed: {e:#}");
        }
    }
}

/// PHASE 8: Periodic persist.
async fn persist_sim_state(sim: &Arc<Mutex<SimState>>, data_dir: &str) {
    let sim_clone = { sim.lock().await.clone() };
    if let Err(e) = crate::simulator::persist::save(&sim_clone, data_dir).await {
        error!("sim persist failed: {e:#}");
    }
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
    deviation_pending: Arc<std::sync::atomic::AtomicBool>,
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

        // Plan F/G: Layer 1/2 absorber state (multi-asset deviation absorption + Tier 2 escalation)
        let mut absorber_state = AbsorberState {
            residual_ticks: 0,
            last_state_change_ts: HashMap::new(),
            settling_ticks: HashMap::new(),
            active_overlay_kw: HashMap::new(),
            correction_is_active: false,
            last_emitted_correction_kw: 0.0,
        };

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
            let overlay_enabled =
                ev_settings_tick.opportunistic_charging_enabled && !session_active;
            if ev_settings_tick.paused_by_active_session != session_active {
                state
                    .set_ev_settings(EvSettings {
                        paused_by_active_session: session_active,
                        ..ev_settings_tick
                    })
                    .await;
            }

            // Lock sim for physics only — no .await inside the block.
            // All async state publishes happen after the lock is released,
            // preventing HTTP handlers from blocking for the full tick cycle.
            let (
                tick_sensor,
                tick_sim_snap,
                tick_envelope,
                cleared_fields,
                pv_clear,
                base_clear,
                residual_kw,
            ) = {
                let mut sim_guard = sim.lock().await;

                // PHASE 1: Apply Behaviour A one-shot injections; collect fields to clear.
                let cleared_fields = apply_sim_injections(&inject, &mut *sim_guard);
                let pv_clear = inject.pv_irradiance.is_some();
                let base_clear = inject.base_load_kw.is_some();

                // PHASE 2: Build setpoints (prev grid for Layer 1; effective capacity; dispatcher)
                // Plan F: capture prev tick net grid for Layer 1 correction.
                // Use signed net: positive = import, negative = export. The plan
                // stores import and export as separate non-negative fields; combining
                // them into a signed value prevents false corrections when the plan
                // intentionally expects export (net_import_kw=0, net_export_kw>0).
                let prev_actual_net_kw = sim_guard.grid.net_power_w / 1000.0;
                let plan_signed_net_kw = plan_snap
                    .as_ref()
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

                // PHASE 3: Layer 1 multi-asset deviation absorber (Tier 1 real-time control).
                let deviation_kw = prev_actual_net_kw - plan_signed_net_kw;
                let residual_kw = controller::absorber::apply_deviation_absorption(
                    &mut absorber_state,
                    deviation_kw,
                    &mut sp_map,
                    &*sim_guard,
                    plan_snap.as_ref(),
                    &profile,
                    now,
                    &event_tx,
                    ev_sess_tick.as_ref(),
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

                // PHASE 5 (in-lock): extract snapshots and mutate history/grid/envelope.
                // All ops are sync — no .await. Lock released at end of this block.
                let (tick_sensor, tick_sim_snap, tick_envelope) =
                    finalize_tick_outputs(&mut *sim_guard, &capacity_snap, now);

                (
                    tick_sensor,
                    tick_sim_snap,
                    tick_envelope,
                    cleared_fields,
                    pv_clear,
                    base_clear,
                    residual_kw,
                )
                // ← sim_guard DROPPED HERE — lock released
            };

            // PHASE 1 (post-lock): clear one-shot inject fields.
            for field in cleared_fields {
                state.clear_inject_field(field).await;
            }
            // pv_irradiance and base_load_kw are one-shots: apply offset once then let it decay.
            if pv_clear {
                state.clear_inject_field("pv_irradiance").await;
            }
            if base_clear {
                state.clear_inject_field("base_load_kw").await;
            }

            // PHASE 5 (post-lock): async state publishes — sensor, shiftable, ledger, envelope.
            let sim_snapshot = publish_sim_tick_result(
                tick_sensor,
                tick_sim_snap,
                tick_envelope,
                plan_snap.as_ref(),
                &state,
                &*trigger_tx,
                &rates_snap,
                dt_s,
                now,
            )
            .await;

            // PHASE 6: Layer 2 — accumulate absorbed residual deviation → DeviceDeviation trigger.
            accumulate_deviation(
                &mut absorber_state,
                residual_kw,
                &profile,
                &*trigger_tx,
                &deviation_pending,
                now,
            );

            // PHASE 7: Periodic measurement reports (T049)
            if report_every_ticks > 0 {
                report_counter += 1;
                if report_counter >= report_every_ticks {
                    report_counter = 0;
                    run_measurement_reports(&state, &sim, &vtn, &ven_name, now).await;
                }
            }

            // PHASE 8: Periodic persist
            persist_counter += 1;
            if persist_counter >= persist_every_ticks {
                persist_counter = 0;
                persist_sim_state(&sim, &data_dir).await;
            }
        }
    })
}
