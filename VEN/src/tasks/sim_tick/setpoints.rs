//! This tick's setpoint decision: the plan's base allocation, then every
//! reactive pass that may change it.
//!
//! What else lived here moved to the ring that owns it -- the state
//! injections to `simulator::inject`, the capacity composition onto
//! `OadrCapacityState` itself, and the comms-loss PV resolver to
//! `controller::comms_loss`. None of the three was scheduling, which is all
//! `tasks/` is for; keeping them here is what kept this file at 196 of its
//! 200 allowed lines.

use chrono::{DateTime, Utc};
use std::collections::HashMap;

use crate::controller;
use crate::controller::SimSnapshot;
use crate::entities::capacity::tightest_capacity_limit;

use super::context::TickContext;
use super::dispatch_override::{
    apply_comms_loss_clamp, apply_dispatch_override, comms_loss_setpoint_bounds_kw,
};

/// PHASE 2: build the plan's base setpoint allocation, then the arbiter's
/// reactive layer on top of it (`controller::arbiter`): deviation correction
/// (`reconcile`, when enabled — else the pre-arbiter `apply_surplus_ev_overlay`
/// path), the DISPATCH_SETPOINT override, the comms-loss clamp, and last the
/// limit-enforcement pass (GB-47, when enabled), which therefore also bounds a
/// dispatch setpoint above a hard import limit while staying inside the
/// comms-loss bounds.
///
/// `live_pv_released_kw`: the PV preview without the arbiter's own curtailment (R-88).
/// `live_pv_kw`/`live_base_load_kw`: this tick's previewed output for the two
/// physics-driven inputs (`SimState::peek_pv_kw`/`peek_base_load_kw`),
/// computed *before* physics runs — so no pass reads a one-tick-stale value.
pub(crate) fn build_tick_setpoints(
    ctx: &TickContext,
    sim_snap: &SimSnapshot,
    thermostat_setpoints_kw: &HashMap<String, f64>,
    now: DateTime<Utc>,
    (live_pv_kw, live_pv_released_kw, live_base_load_kw): (Option<f64>, Option<f64>, Option<f64>),
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
        live_pv_released_kw,
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
