// Synchronous helper functions for the simulator tick.

use chrono::{DateTime, Utc};
use std::collections::HashMap;
use tracing::{debug, warn};

use crate::controller;
use crate::controller::absorber::AbsorberState;
use crate::entities::asset::PlanTrigger;
use crate::entities::capacity::OadrCapacityState;
use crate::entities::plan::{Plan, SiteFlexibilityEnvelope};
use crate::entities::planner_params::AbsorberParams;
use crate::models::SensorSnapshot;
use crate::controller::SimSnapshot;
use crate::simulator::SimState;
use crate::state::SimInjectState;

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
    sim_snap: &SimSnapshot,
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
            sim_snap,
            &effective_capacity,
            inject.heater_setpoint_c,
            now,
            overlay_enabled,
        ),
        None => {
            // No plan yet (startup window). Apply defaults then surplus overlay.
            let mut m: HashMap<String, f64> = sim_snap
                .assets
                .iter()
                .map(|(id, snap)| (id.clone(), snap.default_setpoint_kw))
                .collect();
            controller::dispatcher::apply_surplus_ev_overlay(
                &mut m,
                sim_snap,
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

    // Compute site envelope (pure math — reads snapshot taken above).
    let tick_envelope = controller::envelope::compute_envelope(&tick_sim_snap, now);

    (tick_sensor, tick_sim_snap, tick_envelope)
}

/// PHASE 6: Layer 2 — accumulate absorbed residual deviation → DeviceDeviation trigger.
pub(crate) fn accumulate_deviation(
    absorber_state: &mut AbsorberState,
    residual_kw: f64,
    absorber_params: &AbsorberParams,
    trigger_tx: &tokio::sync::watch::Sender<PlanTrigger>,
    deviation_pending: &std::sync::atomic::AtomicBool,
    _now: DateTime<Utc>,
) {
    // Residual exceeds dead-band: increment sustained deviation counter
    if residual_kw.abs() > absorber_params.dead_band_kw {
        absorber_state.residual_ticks = absorber_state.residual_ticks.saturating_add(1);
        debug!(
            residual_kw,
            dead_band_kw = absorber_params.dead_band_kw,
            residual_ticks = absorber_state.residual_ticks,
            trigger_ticks = absorber_params.deviation_trigger_ticks,
            "layer2: sustained residual deviation tick"
        );
        if absorber_state.residual_ticks >= absorber_params.deviation_trigger_ticks {
            absorber_state.residual_ticks = 0;
            warn!(
                residual_kw,
                dead_band_kw = absorber_params.dead_band_kw,
                trigger_ticks = absorber_params.deviation_trigger_ticks,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::absorber::AbsorberState;
    use crate::entities::planner_params::AbsorberParams;

    fn make_absorber_state() -> AbsorberState {
        AbsorberState {
            residual_ticks: 0,
            last_state_change_ts: HashMap::new(),
            settling_ticks: HashMap::new(),
            active_overlay_kw: HashMap::new(),
            correction_is_active: false,
            last_emitted_correction_kw: 0.0,
        }
    }

    #[test]
    fn accumulate_deviation_fires_after_trigger_ticks() {
        use crate::entities::asset::PlanTrigger;
        let params = AbsorberParams {
            enabled: true,
            dead_band_kw: 0.1,
            dead_band_clearing_ticks: 1,
            deviation_trigger_ticks: 3,
            assets: vec![],
        };
        let (tx, _rx) = tokio::sync::watch::channel(PlanTrigger::Periodic);
        let pending = std::sync::atomic::AtomicBool::new(false);
        let mut state = make_absorber_state();
        let now = chrono::Utc::now();
        // 2 ticks: counter increments but trigger not yet fired
        for _ in 0..2 {
            accumulate_deviation(&mut state, 5.0, &params, &tx, &pending, now);
        }
        assert!(!pending.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(state.residual_ticks, 2);
        // 3rd tick: fires the trigger and resets counter
        accumulate_deviation(&mut state, 5.0, &params, &tx, &pending, now);
        assert!(pending.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(state.residual_ticks, 0);
    }

    #[test]
    fn accumulate_deviation_clears_within_dead_band() {
        let params = AbsorberParams {
            enabled: true,
            dead_band_kw: 1.0,
            dead_band_clearing_ticks: 1,
            deviation_trigger_ticks: 5,
            assets: vec![],
        };
        let (tx, _rx) = tokio::sync::watch::channel(crate::entities::asset::PlanTrigger::Periodic);
        let pending = std::sync::atomic::AtomicBool::new(false);
        let mut state = make_absorber_state();
        state.residual_ticks = 3;
        let now = chrono::Utc::now();
        // residual within dead-band: counter resets
        accumulate_deviation(&mut state, 0.5, &params, &tx, &pending, now);
        assert_eq!(state.residual_ticks, 0);
        assert!(!pending.load(std::sync::atomic::Ordering::Relaxed));
    }
}
