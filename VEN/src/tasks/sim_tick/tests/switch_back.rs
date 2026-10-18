//! Switching from the opportunistic objective back to a planning one leaves no opportunistic
//! memory in what the planning passes read (openspec `opportunistic-objective` D0, spec
//! "Switching back leaves no opportunistic memory"): the opportunistic arm writes no deviation
//! lever, no displacement and no deviation figures, so the first planned tick decides exactly
//! as a VEN that never ran opportunistic, given the same physical state.

use chrono::Duration;

use super::setpoints_golden::{context, plan_with, sim, t0, Scenario};
use crate::entities::planner_params::PlannerObjective;
use crate::simulator::TickInputs;

fn planned() -> crate::entities::plan::Plan {
    plan_with(&[("battery", 2.0), ("ev", 3.0), ("heater", 1.5)])
}

#[test]
fn the_first_planned_tick_after_opportunistic_decides_as_a_fresh_planning_ven() {
    let mut sim = sim();
    let mut opportunistic_plan = planned();
    opportunistic_plan.objective = PlannerObjective::Opportunistic;
    let scenario = Scenario {
        deviation: true,
        ..Scenario::new("switch_back", Some(opportunistic_plan))
    };
    let mut ctx = context(&scenario, &sim);
    ctx.opportunistic = Some(Default::default());
    let mut now = t0();
    for _ in 0..60 {
        let snap = sim.to_sim_snapshot();
        let (live, released) = super::feeds::live_previews(&sim, &ctx, &snap, now, 1.0);
        let thermostat = sim.thermostat_setpoints_kw(None);
        let o = super::setpoints::build_tick_setpoints(
            &ctx,
            &snap,
            &thermostat,
            now,
            (&live, &released),
        );
        assert!(o.opportunistic.is_some(), "the opportunistic arm ran");
        assert_eq!(o.active_lever, None, "no deviation lever written");
        assert!(
            o.displaced_kw_by_asset.is_empty(),
            "no displacement for the residual tracker"
        );
        assert_eq!(
            (o.net_kw, o.dev_kw, o.dev_without_correction_kw),
            (None, None, None)
        );
        sim.tick(TickInputs::new(1.0, now, o.setpoints.clone()));
        ctx.incumbent_lever = o.active_lever.clone();
        ctx.limit_incumbent_lever = o.limit.as_ref().and_then(|l| l.active_lever.clone());
        ctx.bridge_kw_by_asset = super::arbiter_glue::both_passes_bridge_kw(&o);
        now += Duration::seconds(1);
    }

    // Switch back: the carried state is what a fresh planning VEN in this physical state has.
    let back = Scenario {
        deviation: true,
        ..Scenario::new("switch_back", Some(planned()))
    };
    let mut switched = context(&back, &sim);
    switched.incumbent_lever = ctx.incumbent_lever.clone();
    switched.limit_incumbent_lever = ctx.limit_incumbent_lever.clone();
    switched.bridge_kw_by_asset = ctx.bridge_kw_by_asset.clone();
    let fresh = context(&back, &sim);
    let snap = sim.to_sim_snapshot();
    let (live, released) = super::feeds::live_previews(&sim, &switched, &snap, now, 1.0);
    let thermostat = sim.thermostat_setpoints_kw(None);
    let after = super::setpoints::build_tick_setpoints(
        &switched,
        &snap,
        &thermostat,
        now,
        (&live, &released),
    );
    let reference =
        super::setpoints::build_tick_setpoints(&fresh, &snap, &thermostat, now, (&live, &released));
    assert_eq!(after, reference);
    assert!(after.opportunistic.is_none());
}
