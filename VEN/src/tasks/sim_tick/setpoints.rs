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
use crate::entities::capacity::effective_capacity_limit;

use super::context::TickContext;

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
    (live_kw, live_released_kw): (
        &controller::arbiter::LiveKwByAsset,
        &controller::arbiter::LiveKwByAsset,
    ),
) -> controller::arbiter::ArbiterOutcome {
    use crate::entities::planner_params::PlannerObjective;
    use controller::arbiter::limit;
    let plan_snap = ctx.plan_snap.as_ref();
    let base_sp = match plan_snap {
        Some(plan) => {
            controller::dispatcher::build_setpoints(plan, sim_snap, thermostat_setpoints_kw, now)
        }
        None => sim_snap.default_setpoints_kw(),
    };
    let alert_active = lab_core::time_window::any_covering(&ctx.alert_windows, now);
    // A sim-injected import limit stands in only while no VTN limit is in force.
    use crate::entities::capacity_curve::CommitmentDirection::Import;
    let capacity_limit_kw =
        effective_capacity_limit(&ctx.capacity_schedule, &ctx.inject, Import, now, now)
            .map(|l| l.limit_kw);
    let hard_limit_kw = limit::hard_import_limit_kw(capacity_limit_kw, alert_active)
        .filter(|_| ctx.limit_enforcement_enabled);
    // The opportunistic arm follows no plan allocation anywhere (D1): not in the base, not in
    // the EV overlays, not as the slot the levers are priced in. So the forecast, which has no
    // plan yet when it steps, decides as the live tick does.
    let followed_plan = plan_snap.filter(|_| ctx.opportunistic.is_none());
    let ev_overlay = controller::arbiter::ev_overlays(sim_snap, followed_plan, now, |id| {
        ctx.overlay_enabled.of(id)
    });
    let tick = controller::arbiter::ArbiterTick {
        sim: sim_snap,
        plan_slot: followed_plan.and_then(|p| p.current_slot(now)),
        objective: plan_snap.map_or(PlannerObjective::MinCost, |p| p.objective),
        ev_overlay: &ev_overlay,
        live_kw,
        live_released_kw: Some(live_released_kw),
        alert_active,
        limit_target_kw: limit::limit_target_kw(
            hard_limit_kw,
            ctx.limit_incumbent_lever.as_deref(),
        ),
        prev_bridge_kw_by_asset: &ctx.bridge_kw_by_asset,
    };

    let stages = controller::arbiter::DispatchStages {
        dispatch_windows: &ctx.dispatch_windows,
        alert_windows: &ctx.alert_windows,
        comms_loss: ctx.comms_loss,
        limit_incumbent_lever: ctx.limit_incumbent_lever.as_deref(),
    };
    if let Some(requests) = &ctx.opportunistic {
        // The opportunistic objective: no plan allocation is followed (D1, D5); the one step
        // the forecast runs too (D6) decides from this tick's balance and bounds it.
        let defaults =
            controller::dispatcher::build_setpoints_without_plan(sim_snap, thermostat_setpoints_kw);
        let ev_switch = |id: &str| ctx.overlay_enabled.of(id);
        return controller::arbiter::opportunistic_step(
            &tick, defaults, requests, ev_switch, now, &stages,
        );
    }
    let outcome = if ctx.deviation_arbiter_enabled {
        controller::arbiter::reconcile(&tick, &base_sp, ctx.incumbent_lever.as_deref())
    } else {
        let mut sp = base_sp;
        controller::dispatcher::apply_surplus_ev_overlay(&mut sp, sim_snap, &ev_overlay, live_kw);
        controller::arbiter::ArbiterOutcome {
            setpoints: sp,
            ..Default::default()
        }
    };

    controller::arbiter::dispatch_stages(outcome, &tick, now, &stages)
}
