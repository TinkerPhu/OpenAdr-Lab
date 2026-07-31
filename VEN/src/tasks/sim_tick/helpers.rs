// Synchronous helper functions for the simulator tick.

use chrono::{DateTime, Utc};
use std::collections::HashMap;

use crate::controller;
use crate::controller::SimSnapshot;
use crate::entities::capacity::OadrCapacityState;
use crate::entities::plan::{Plan, SiteFlexibilityEnvelope};
use crate::entities::sim_inject::SimInjectState;
use crate::models::SensorSnapshot;
use crate::simulator::SimState;

use super::dispatch_override::apply_dispatch_override;

/// PHASE 1: Apply Behaviour A one-shot state injections to the simulator.
/// Returns a list of field names that were applied and should be cleared.
pub(crate) fn apply_sim_injections(
    inject: &SimInjectState,
    sim: &mut SimState,
) -> Vec<&'static str> {
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

/// Compose effective capacity: inject grid limits only when no VTN event is active.
/// Used by the PV generation-limit resolver (`tasks/sim_tick/tick.rs`) so it sees the
/// same sim-injected overrides (`grid_import/export_limit_kw`), not just the raw
/// VTN-driven `OadrCapacityState`.
pub(crate) fn effective_capacity(
    capacity_snap: &OadrCapacityState,
    inject: &SimInjectState,
) -> OadrCapacityState {
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
    effective_capacity
}

/// PHASE 2: Compose effective capacity, build the plan's base setpoint
/// allocation, then run the deviation arbiter's reactive adjustment layer
/// (`controller::arbiter::reconcile`) on top of it — the single owner of
/// every reactive (non-plan, non-VTN-override) actuator write per tick
/// (`openspec/changes/deviation-arbiter/`).
///
/// `live_pv_kw`/`live_base_load_kw`: this tick's previewed output for the two
/// physics-driven inputs (`SimState::peek_pv_kw`/`peek_base_load_kw`),
/// computed *before* physics runs — passed through so the arbiter's deviation
/// calculation never reads a one-tick-stale snapshot for either.
///
/// When `deviation_arbiter_enabled` is `false`, takes the exact pre-arbiter
/// code path (`apply_surplus_ev_overlay` inline) — fully reversible rollout.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_tick_setpoints(
    sim_snap: &SimSnapshot,
    plan_snap: Option<&Plan>,
    inject: &SimInjectState,
    overlay_enabled: bool,
    now: DateTime<Utc>,
    dispatch_windows: &[crate::entities::capacity::DispatchWindow],
    alert_windows: &[crate::entities::capacity::AlertWindow],
    live_pv_kw: Option<f64>,
    live_base_load_kw: Option<f64>,
    deviation_arbiter_enabled: bool,
    incumbent_lever: Option<&str>,
) -> controller::arbiter::ArbiterOutcome {
    let base_sp = match plan_snap {
        Some(plan) => {
            controller::dispatcher::build_setpoints(plan, sim_snap, inject.heater_setpoint_c, now)
        }
        None => sim_snap
            .assets
            .iter()
            .map(|(id, snap)| (id.clone(), snap.default_setpoint_kw))
            .collect(),
    };

    let mut outcome = if deviation_arbiter_enabled {
        let plan_has_ev_allocation =
            plan_snap.is_some_and(|p| controller::dispatcher::plan_has_ev_allocation(p, now));
        let plan_slot =
            plan_snap.and_then(|p| p.slots.iter().find(|s| s.start <= now && now < s.end));
        let objective = plan_snap
            .map(|p| p.objective)
            .unwrap_or(crate::entities::planner_params::PlannerObjective::MinCost);
        controller::arbiter::reconcile(
            sim_snap,
            &base_sp,
            plan_slot,
            objective,
            plan_has_ev_allocation,
            overlay_enabled,
            live_pv_kw,
            live_base_load_kw,
            incumbent_lever,
        )
    } else {
        let mut sp = base_sp;
        let plan_has_ev_allocation =
            plan_snap.is_some_and(|p| controller::dispatcher::plan_has_ev_allocation(p, now));
        controller::dispatcher::apply_surplus_ev_overlay(
            &mut sp,
            sim_snap,
            plan_has_ev_allocation,
            overlay_enabled,
            live_pv_kw,
        );
        controller::arbiter::ArbiterOutcome {
            setpoints: sp,
            ..Default::default()
        }
    };

    apply_dispatch_override(
        &mut outcome.setpoints,
        sim_snap,
        now,
        dispatch_windows,
        alert_windows,
        live_pv_kw,
    );
    outcome
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
