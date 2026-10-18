//! The guard of the opportunistic forecast (openspec `opportunistic-objective` 7.4, D6): one
//! forecast step and one live tick, given the same asset states, PV, base load, limits and
//! requests, command the same setpoints. The forecast runs the live tick's own decision
//! (`controller::arbiter::opportunistic_step`); this pins that nothing around it differs.

use std::collections::HashMap;

use super::setpoints_golden::{context, plan_with, sim, t0, Scenario};
use crate::controller::arbiter::{OpportunisticRequests, OverlayEnabled, RequestTargets};
use crate::entities::design_vocabulary::NonPlanningDispatch;
use crate::entities::planner_params::PlannerObjective;
use crate::simulator::opportunistic_forecast::{slot_decision, ForecastSite, SlotRequests};

struct Case {
    name: &'static str,
    pv_kw: f64,
    base_load_kw: f64,
    import_limit_kw: Option<f64>,
    alert: bool,
    requests: OpportunisticRequests,
}

fn start_now(asset_id: &str) -> OpportunisticRequests {
    OpportunisticRequests {
        dispatch_by_asset: HashMap::from([(asset_id.to_string(), NonPlanningDispatch::StartNow)]),
        targets: RequestTargets::default(),
    }
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "pv_surplus_no_request",
            pv_kw: 4.0,
            base_load_kw: 0.5,
            import_limit_kw: None,
            alert: false,
            requests: Default::default(),
        },
        Case {
            name: "ev_start_now_under_a_limit",
            pv_kw: 0.0,
            base_load_kw: 0.5,
            import_limit_kw: Some(3.0),
            alert: false,
            requests: start_now("ev"),
        },
        Case {
            name: "heater_start_now_with_surplus",
            pv_kw: 2.0,
            base_load_kw: 0.5,
            import_limit_kw: None,
            alert: false,
            requests: start_now("heater"),
        },
        Case {
            name: "alert_window",
            pv_kw: 0.0,
            base_load_kw: 1.5,
            import_limit_kw: None,
            alert: true,
            requests: Default::default(),
        },
    ]
}

#[test]
fn a_forecast_step_commands_what_a_live_tick_commands() {
    let sim = sim();
    let now = t0();
    let dt_s = 1.0;
    for case in cases() {
        let mut plan = plan_with(&[("battery", 2.0), ("ev", 3.0)]);
        plan.objective = PlannerObjective::Opportunistic;
        let scenario = Scenario {
            import_limit_kw: case.import_limit_kw,
            alert: case.alert,
            ..Scenario::new("forecast_guard", Some(plan))
        };
        let mut ctx = context(&scenario, &sim);
        ctx.feeds.measured_kw = HashMap::from([
            ("pv".to_string(), case.pv_kw),
            ("base_load".to_string(), case.base_load_kw),
        ]);
        ctx.opportunistic = Some(case.requests.clone());

        let snap = sim.to_sim_snapshot();
        let (live, released) = super::feeds::live_previews(&sim, &ctx, &snap, now, dt_s);
        let thermostat = sim.thermostat_setpoints_kw(None);
        let live_outcome = super::setpoints::build_tick_setpoints(
            &ctx,
            &snap,
            &thermostat,
            now,
            (&live, &released),
        );

        let slot = SlotRequests {
            requests: case.requests.clone(),
            ev_surplus_switch_on: ctx.overlay_enabled.clone(),
            ev_departure_by_asset: HashMap::new(),
        };
        let requests_at = |_| slot.clone();
        let site = ForecastSite {
            capacity_schedule: &ctx.capacity_schedule,
            dispatch_windows: &ctx.dispatch_windows,
            alert_windows: &ctx.alert_windows,
            limit_enforcement_enabled: ctx.limit_enforcement_enabled,
            requests_at: &requests_at,
        };
        let (forecast_setpoints, forecast_emergency, _) =
            slot_decision(&sim, &ctx.feeds, &slot, &site, now, dt_s, None);

        assert_eq!(
            forecast_setpoints, live_outcome.setpoints,
            "{}: setpoints",
            case.name
        );
        let live_emergency = crate::controller::arbiter::heater_emergency_modes(
            &ctx.inject,
            &live_outcome.heater_emergency_mode,
            &snap,
        );
        assert_eq!(
            forecast_emergency, live_emergency,
            "{}: heater emergency modes",
            case.name
        );
    }
}

/// The guard compares like with like only if the overlay switches agree; both sides read them
/// from `OverlayEnabled`, whose answer for an EV with no record is the default settings'.
#[test]
fn an_ev_without_settings_reads_the_default_switch_on_both_sides() {
    let empty = OverlayEnabled::default();
    assert_eq!(
        empty.of("ev"),
        crate::entities::ev_settings::EvSettings::default().overlay_enabled(false)
    );
}
