// Synchronous helper functions for the simulator tick.

use chrono::{DateTime, Utc};
use std::collections::HashMap;

use crate::controller;
use crate::controller::SimSnapshot;
use crate::entities::capacity::{tightest_capacity_limit, OadrCapacityState};
use crate::entities::plan::Plan;
use crate::entities::sim_inject::SimInjectState;
use crate::simulator::SimState;

use super::context::{CommsLossState, TickContext};
use super::dispatch_override::{
    apply_comms_loss_clamp, apply_dispatch_override, comms_loss_setpoint_bounds_kw,
};

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

/// PHASE 1b: resolve the PV generation limit from capacity/plan/arbiter/manual/
/// comms-loss sources, composing `effective_capacity` above with
/// `controller::dispatcher::resolve_pv_generation_limit_kw`. `sim_snap` is only
/// needed to read the PV asset's `inverter_max_kw` ceiling for the comms-loss
/// candidate (R-59).
pub(crate) fn resolve_pv_limit(
    sim_snap: &SimSnapshot,
    plan_snap: Option<&Plan>,
    capacity_snap: &OadrCapacityState,
    inject: &SimInjectState,
    now: DateTime<Utc>,
    arbiter_tighten_kw: Option<f64>,
    comms_loss: Option<CommsLossState>,
) -> controller::dispatcher::ResolvedPvGenerationLimit {
    let capacity = effective_capacity(capacity_snap, inject);
    let comms_loss_limit_kw = comms_loss.filter(|c| c.active).and_then(|c| {
        sim_snap
            .assets
            .get(crate::ids::ASSET_PV)
            .and_then(|s| s.val("inverter_max_kw"))
            .map(|max_kw| c.max_power_pct * max_kw)
    });
    controller::dispatcher::resolve_pv_generation_limit_kw(
        plan_snap,
        &capacity,
        now,
        arbiter_tighten_kw,
        inject.pv_generation_limit_kw,
        comms_loss_limit_kw,
    )
}

/// PHASE 2: build the plan's base setpoint allocation, then the arbiter's
/// reactive layer on top of it (`controller::arbiter`): deviation correction
/// (`reconcile`, when enabled — else the pre-arbiter `apply_surplus_ev_overlay`
/// path), the DISPATCH_SETPOINT override, the comms-loss clamp, and last the
/// limit-enforcement pass (GB-47, when enabled), which therefore also bounds a
/// dispatch setpoint above a hard import limit while staying inside the
/// comms-loss bounds.
///
/// `live_pv_kw`/`live_base_load_kw`: this tick's previewed output for the two
/// physics-driven inputs (`SimState::peek_pv_kw`/`peek_base_load_kw`),
/// computed *before* physics runs — so no pass reads a one-tick-stale value.
pub(crate) fn build_tick_setpoints(
    ctx: &TickContext,
    sim_snap: &SimSnapshot,
    thermostat_setpoints_kw: &HashMap<String, f64>,
    now: DateTime<Utc>,
    (live_pv_kw, live_base_load_kw): (Option<f64>, Option<f64>),
) -> controller::arbiter::ArbiterOutcome {
    use crate::entities::planner_params::PlannerObjective;
    use controller::arbiter::limit;
    let plan_snap = ctx.plan_snap.as_ref();
    let base_sp = match plan_snap {
        Some(plan) => {
            controller::dispatcher::build_setpoints(plan, sim_snap, thermostat_setpoints_kw, now)
        }
        None => sim_snap
            .assets
            .iter()
            .map(|(id, snap)| (id.clone(), snap.default_setpoint_kw))
            .collect(),
    };
    let alert_active = lab_core::time_window::any_covering(&ctx.alert_windows, now);
    // A sim-injected import limit stands in only while no VTN limit is in force.
    use crate::entities::capacity_curve::CommitmentDirection::Import;
    let capacity_limit_kw = tightest_capacity_limit(&ctx.capacity_schedule, Import, now, now)
        .map(|l| l.limit_kw)
        .or(ctx.inject.grid_import_limit_kw);
    let hard_limit_kw = limit::hard_import_limit_kw(capacity_limit_kw, alert_active)
        .filter(|_| ctx.limit_enforcement_enabled);
    let tick = controller::arbiter::ArbiterTick {
        sim: sim_snap,
        plan_slot: plan_snap.and_then(|p| p.current_slot(now)),
        objective: plan_snap.map_or(PlannerObjective::MinCost, |p| p.objective),
        plan_has_ev_allocation: plan_snap
            .is_some_and(|p| controller::dispatcher::plan_has_ev_allocation(p, now)),
        overlay_enabled: ctx.overlay_enabled,
        live_pv_kw,
        live_base_load_kw,
        alert_active,
        limit_target_kw: limit::limit_target_kw(
            hard_limit_kw,
            ctx.limit_incumbent_lever.as_deref(),
        ),
    };

    let mut outcome = if ctx.deviation_arbiter_enabled {
        controller::arbiter::reconcile(&tick, &base_sp, ctx.incumbent_lever.as_deref())
    } else {
        let mut sp = base_sp;
        controller::dispatcher::apply_surplus_ev_overlay(
            &mut sp,
            sim_snap,
            tick.plan_has_ev_allocation,
            ctx.overlay_enabled,
            live_pv_kw,
        );
        controller::arbiter::ArbiterOutcome {
            setpoints: sp,
            ..Default::default()
        }
    };

    let sp = &mut outcome.setpoints;
    let (dispatch, alerts) = (&ctx.dispatch_windows, &ctx.alert_windows);
    apply_dispatch_override(
        sp,
        sim_snap,
        now,
        dispatch,
        alerts,
        live_pv_kw,
        live_base_load_kw,
    );
    apply_comms_loss_clamp(sp, sim_snap, ctx.comms_loss);
    let bounds_kw = comms_loss_setpoint_bounds_kw(sim_snap, ctx.comms_loss);
    // A deviation-pass PV curtailment lowers generation, so the limit pass
    // projects PV after it.
    let pv_tighten_kw = outcome.pv_generation_limit_tighten_kw.unwrap_or(0.0);
    let limit_tick = controller::arbiter::ArbiterTick {
        live_pv_kw: live_pv_kw.map(|pv_kw| (pv_kw + pv_tighten_kw).min(0.0)),
        ..tick
    };
    let incumbent = ctx.limit_incumbent_lever.as_deref();
    outcome.limit = limit::enforce_import_limit(&limit_tick, sp, incumbent, &bounds_kw);
    // Limit-pass Curtail (alerts only) overrides a deviation-pass Absorb.
    if let Some(mode) = outcome.limit.as_ref().and_then(|l| l.heater_emergency_mode) {
        outcome.heater_emergency_mode = Some(mode);
    }
    outcome
}
