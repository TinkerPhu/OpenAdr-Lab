// Helpers extracted from VEN/src/tasks/sim_tick.rs

use chrono::{DateTime, Utc};
use std::collections::HashMap;
use crate::assets::AssetConfig;
use crate::controller;
use crate::entities::asset::PlanTrigger;
use crate::entities::capacity::OadrCapacityState;
use crate::entities::plan::Plan;
use crate::entities::plan::SiteFlexibilityEnvelope;
use crate::entities::tariff_snapshot::TariffSnapshot;
use crate::models::SensorSnapshot;
use crate::simulator::SimState;
use crate::simulator::{AssetEntry, SimSnapshot};
use crate::state::{AppState, EvSettings, SimInjectState};
use crate::profile::Profile;
use crate::controller::absorber::AbsorberState;

/// PHASE 1: Apply Behaviour A one-shot state injections to the simulator.
/// Returns a list of field names that were applied and should be cleared.
pub(crate) fn apply_sim_injections(inject: &SimInjectState, sim: &mut SimState) -> Vec<&'static str> {
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
pub(crate) fn build_tick_setpoints(
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

/// PHASE 5 in-lock tail: extract snapshots, push history, update grid asset, compute envelope.
/// Returns the 3-tuple needed for post-lock async state publishing.
pub(crate) fn finalize_tick_outputs(
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
pub(crate) fn accumulate_deviation(
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
