//! GB-47 limit enforcement: the arbiter's second pass keeps site import under
//! an active hard limit whatever the plan says. Cases mirror the 2026-09 fleet
//! campaign (`docs/history/fleet_run_journal.md`). Shares the snapshot helpers
//! of the parent `arbiter_tests` module.

use super::*;
use crate::controller::arbiter::limit::{
    enforce_import_limit, hard_import_limit_kw, limit_target_kw, SetpointBoundsKw, LIMIT_MARGIN_KW,
    LIMIT_RELEASE_HYSTERESIS_KW,
};

fn tick<'a>(
    sim: &'a SimSnapshot,
    plan_slot: Option<&'a PlanTimeSlot>,
    live_base_load_kw: f64,
    limit_target_kw: Option<f64>,
    alert_active: bool,
) -> ArbiterTick<'a> {
    ArbiterTick {
        sim,
        plan_slot,
        objective: PlannerObjective::MinCost,
        plan_has_ev_allocation: false,
        overlay_enabled: true,
        live_pv_kw: None,
        live_base_load_kw: Some(live_base_load_kw),
        alert_active,
        limit_target_kw,
    }
}

fn no_bounds() -> SetpointBoundsKw {
    SetpointBoundsKw::new()
}

fn projected(t: &ArbiterTick, sp: &StdHashMap<String, f64>) -> f64 {
    projected_net_kw(t.sim, sp, t.live_pv_kw, t.live_base_load_kw)
}

// ── the shared target ───────────────────────────────────────────────────────

#[test]
fn hard_import_limit_kw_is_zero_during_an_alert_else_the_capacity_limit() {
    assert_eq!(hard_import_limit_kw(Some(3.0), true), Some(0.0));
    assert_eq!(hard_import_limit_kw(None, true), Some(0.0));
    assert_eq!(hard_import_limit_kw(Some(3.0), false), Some(3.0));
    assert_eq!(hard_import_limit_kw(None, false), None);
}

#[test]
fn limit_target_kw_sits_a_margin_below_and_lower_after_a_switching_lever() {
    assert_eq!(limit_target_kw(None, Some("heater_pause")), None);
    let fresh = limit_target_kw(Some(3.0), None).unwrap();
    let stage_shed = limit_target_kw(Some(3.0), Some("heater_pause")).unwrap();
    let battery = limit_target_kw(Some(3.0), Some("battery")).unwrap();
    assert!((fresh - (3.0 - LIMIT_MARGIN_KW)).abs() < 1e-9);
    assert!((stage_shed - (3.0 - LIMIT_MARGIN_KW - LIMIT_RELEASE_HYSTERESIS_KW)).abs() < 1e-9);
    assert_eq!(
        battery, fresh,
        "the battery adjusts continuously — no release hysteresis"
    );
}

#[test]
fn deviation_kw_steers_to_the_limit_target_when_the_plan_exceeds_it() {
    let slot = test_slot(0.2, 0.2, 2.0, 0.0, 0.0, 0.08);
    assert!((deviation_kw(&slot, 2.0, Some(1.4)) - 0.6).abs() < 1e-9);
    assert!((deviation_kw(&slot, 2.0, Some(5.0))).abs() < 1e-9);
    assert!((deviation_kw(&slot, 2.0, None)).abs() < 1e-9);
}

// ── campaign cases ──────────────────────────────────────────────────────────

#[test]
fn enforce_import_limit_sheds_a_planned_heater_stage_inside_the_cap() {
    // ven-10: the timed-out plan put a heater stage inside a 1.5 kW cap
    // (1.5 + 0.47 base). The stage must go, not be rounded back up.
    let sim = make_sim(vec![
        ("heater", heater_snap(20.0, 18.0, 23.0, 23.0)),
        ("base_load", base_snap(0.47)),
    ]);
    let slot = test_slot(0.25, 0.08, 1.97, 0.0, 0.0, 0.08);
    let t = tick(
        &sim,
        Some(&slot),
        0.47,
        limit_target_kw(Some(1.5), None),
        false,
    );
    let mut sp = StdHashMap::from([("heater".to_string(), 1.5)]);
    let out = enforce_import_limit(&t, &mut sp, None, &no_bounds()).unwrap();
    assert_eq!(sp["heater"], 0.0);
    assert_eq!(out.active_lever, Some("heater_pause"));
    assert!(out.excess_kw > 0.5);
    assert_eq!(out.unresolved_kw, 0.0);
    assert!(projected(&t, &sp) <= 1.5);
}

#[test]
fn enforce_import_limit_discharges_the_battery_for_an_unforecast_base_load() {
    // ven-1: real base load 2.3 kW under a 1.0 kW cap, battery idle per plan.
    let sim = make_sim(vec![
        ("battery", battery_snap(0.0, 0.5)),
        ("base_load", base_snap(0.4)),
    ]);
    let slot = test_slot(0.25, 0.08, 0.4, 0.0, 0.0, 0.08);
    let t = tick(
        &sim,
        Some(&slot),
        2.3,
        limit_target_kw(Some(1.0), None),
        false,
    );
    let mut sp = StdHashMap::from([("battery".to_string(), 0.0)]);
    let out = enforce_import_limit(&t, &mut sp, None, &no_bounds()).unwrap();
    assert_eq!(out.active_lever, Some("battery"));
    assert!((sp["battery"] - -(2.3 - (1.0 - LIMIT_MARGIN_KW))).abs() < 1e-6);
    assert!(projected(&t, &sp) <= 1.0);
    assert!((out.adjusted_kw_by_asset["battery"] - sp["battery"]).abs() < 1e-9);
}

#[test]
fn enforce_import_limit_reduces_charging_the_plan_itself_scheduled() {
    // The deviation pass never touches a planned EV allocation; a hard limit does.
    let sim = make_sim(vec![
        ("ev", ev_snap(7.0, 0.4, 0.8, true)),
        ("base_load", base_snap(0.5)),
    ]);
    let slot = test_slot(0.25, 0.08, 7.5, 0.0, 0.0, 0.08);
    let mut t = tick(
        &sim,
        Some(&slot),
        0.5,
        limit_target_kw(Some(3.0), None),
        false,
    );
    t.plan_has_ev_allocation = true;
    t.overlay_enabled = false;
    let mut sp = StdHashMap::from([("ev".to_string(), 7.0)]);
    enforce_import_limit(&t, &mut sp, None, &no_bounds()).unwrap();
    assert!((sp["ev"] - (3.0 - LIMIT_MARGIN_KW - 0.5)).abs() < 1e-6);
}

#[test]
fn enforce_import_limit_discharges_the_battery_under_max_revenue_too() {
    let sim = make_sim(vec![
        ("battery", battery_snap(0.0, 0.5)),
        ("base_load", base_snap(2.0)),
    ]);
    let slot = test_slot(0.25, 0.08, 2.0, 0.0, 0.0, 0.08);
    let mut t = tick(
        &sim,
        Some(&slot),
        2.0,
        limit_target_kw(Some(1.0), None),
        false,
    );
    t.objective = PlannerObjective::MaxRevenue;
    let mut sp = StdHashMap::from([("battery".to_string(), 0.0)]);
    enforce_import_limit(&t, &mut sp, None, &no_bounds()).unwrap();
    assert!(sp["battery"] < -1.0, "got {}", sp["battery"]);
}

#[test]
fn enforce_import_limit_works_before_the_first_plan() {
    let sim = make_sim(vec![
        ("battery", battery_snap(0.0, 0.5)),
        ("base_load", base_snap(2.0)),
    ]);
    let t = tick(&sim, None, 2.0, limit_target_kw(Some(1.0), None), false);
    let mut sp = StdHashMap::from([("battery".to_string(), 0.0)]);
    let out = enforce_import_limit(&t, &mut sp, None, &no_bounds()).unwrap();
    assert_eq!(out.active_lever, Some("battery"));
    assert!(projected(&t, &sp) <= 1.0);
}

#[test]
fn enforce_import_limit_cuts_back_battery_charging_commanded_above_the_cap() {
    // e.g. a DISPATCH_SETPOINT override asked for more import than the cap allows.
    let sim = make_sim(vec![
        ("battery", battery_snap(4.0, 0.5)),
        ("base_load", base_snap(0.5)),
    ]);
    let t = tick(&sim, None, 0.5, limit_target_kw(Some(2.0), None), false);
    let mut sp = StdHashMap::from([("battery".to_string(), 4.0)]);
    enforce_import_limit(&t, &mut sp, None, &no_bounds()).unwrap();
    assert!((sp["battery"] - (2.0 - LIMIT_MARGIN_KW - 0.5)).abs() < 1e-6);
}

#[test]
fn enforce_import_limit_stays_inside_the_comms_loss_bounds_and_reports_the_rest() {
    let sim = make_sim(vec![
        ("battery", battery_snap(0.0, 0.5)),
        ("base_load", base_snap(3.0)),
    ]);
    let t = tick(&sim, None, 3.0, limit_target_kw(Some(1.0), None), false);
    let bounds = SetpointBoundsKw::from([("battery".to_string(), (-1.0, 1.0))]);
    let mut sp = StdHashMap::from([("battery".to_string(), 0.0)]);
    let out = enforce_import_limit(&t, &mut sp, None, &bounds).unwrap();
    assert!((sp["battery"] - -1.0).abs() < 1e-9);
    assert!((out.unresolved_kw - (3.0 - 1.0 - (1.0 - LIMIT_MARGIN_KW))).abs() < 1e-6);
}

// ── heater safety: alerts may curtail emergency heat, capacity limits may not ─

#[test]
fn enforce_import_limit_curtails_emergency_heat_during_an_alert() {
    // 17 °C < temp_min 18: the thermostat forces 3 kW. An alert may override it.
    let sim = make_sim(vec![
        ("heater", heater_snap(17.0, 18.0, 23.0, 23.0)),
        ("base_load", base_snap(0.5)),
    ]);
    let slot = test_slot(0.25, 0.08, 0.5, 0.0, 0.0, 0.08);
    let hard = hard_import_limit_kw(Some(3.0), true);
    let t = tick(&sim, Some(&slot), 0.5, limit_target_kw(hard, None), true);
    let mut sp = StdHashMap::from([("heater".to_string(), 1.5)]);
    let out = enforce_import_limit(&t, &mut sp, None, &no_bounds()).unwrap();
    assert_eq!(out.heater_emergency_mode, Some((true, false)), "Curtail");
    assert_eq!(sp["heater"], 0.0, "and no planned stage either");
}

#[test]
fn enforce_import_limit_never_curtails_emergency_heat_for_a_capacity_limit() {
    // Same forced heat under a plain capacity limit: the heater's comfort floor
    // stands, nothing else can shed, and the excess is reported, not hidden.
    let sim = make_sim(vec![
        ("heater", heater_snap(17.0, 18.0, 23.0, 23.0)),
        ("base_load", base_snap(0.5)),
    ]);
    let slot = test_slot(0.95, 0.95, 0.5, 0.0, 0.0, 0.08); // penalty-inflated cost
    let t = tick(
        &sim,
        Some(&slot),
        0.5,
        limit_target_kw(Some(1.0), None),
        false,
    );
    let mut sp = StdHashMap::from([("heater".to_string(), 0.0)]);
    let out = enforce_import_limit(&t, &mut sp, None, &no_bounds()).unwrap();
    assert_eq!(out.heater_emergency_mode, None);
    assert!((out.unresolved_kw - (3.5 - (1.0 - LIMIT_MARGIN_KW))).abs() < 1e-6);
}

#[test]
fn heater_curtail_is_offered_only_during_an_alert_whatever_the_marginal_cost() {
    let sim = make_sim(vec![("heater", heater_snap(17.0, 18.0, 23.0, 23.0))]);
    let penalty_slot = test_slot(0.95, 0.95, 5.0, 0.0, 0.0, 0.08);
    assert!(heater_emergency_lever(&sim, Some(&penalty_slot), 1.0, false, false).is_none());
    let routine_slot = test_slot(0.20, 0.20, 5.0, 0.0, 0.0, 0.08);
    assert!(heater_emergency_lever(&sim, Some(&routine_slot), 1.0, false, true).is_some());
    assert!(heater_emergency_lever(&sim, None, 1.0, false, true).is_some());
}

// ── hysteresis and stability ────────────────────────────────────────────────

#[test]
fn enforce_import_limit_keeps_a_shed_stage_off_until_it_fits_with_room_to_spare() {
    // 1.5 heater + 0.35 base = 1.85 under a 2.0 cap: fits a fresh pass (target
    // 1.9), but not one that shed the stage last tick (target 1.7).
    let sim = make_sim(vec![
        ("heater", heater_snap(20.0, 18.0, 23.0, 23.0)),
        ("base_load", base_snap(0.35)),
    ]);
    let mut sp_fresh = StdHashMap::from([("heater".to_string(), 1.5)]);
    let t_fresh = tick(&sim, None, 0.35, limit_target_kw(Some(2.0), None), false);
    let fresh = enforce_import_limit(&t_fresh, &mut sp_fresh, None, &no_bounds()).unwrap();
    assert_eq!(sp_fresh["heater"], 1.5);
    assert_eq!(fresh.active_lever, None);

    let mut sp_engaged = StdHashMap::from([("heater".to_string(), 1.5)]);
    let t_engaged = tick(
        &sim,
        None,
        0.35,
        limit_target_kw(Some(2.0), Some("heater_pause")),
        false,
    );
    let engaged = enforce_import_limit(
        &t_engaged,
        &mut sp_engaged,
        Some("heater_pause"),
        &no_bounds(),
    )
    .unwrap();
    assert_eq!(sp_engaged["heater"], 0.0);
    assert_eq!(engaged.active_lever, Some("heater_pause"));
}

#[test]
fn both_passes_together_settle_under_a_persistent_unforecast_load() {
    // Deviation correction and limit enforcement both on, 10 ticks of a
    // constant 2.3 kW base load that a timed-out plan scheduled in full, above
    // a 1.0 kW cap: the battery settles and the site stays under the cap on
    // every tick — the passes share one target instead of fighting.
    let slot = test_slot(0.25, 0.08, 2.3, 0.0, 0.0, 0.08);
    let base_setpoints = StdHashMap::from([("battery".to_string(), 0.0)]);
    let mut battery_kw = 0.0_f64;
    let mut dev_incumbent: Option<&'static str> = None;
    let mut limit_incumbent: Option<&'static str> = None;
    let mut history = Vec::new();
    for _ in 0..10 {
        let sim = make_sim(vec![
            ("battery", battery_snap(battery_kw, 0.5)),
            ("base_load", base_snap(2.3)),
        ]);
        let target = limit_target_kw(Some(1.0), limit_incumbent);
        let t = tick(&sim, Some(&slot), 2.3, target, false);
        let mut outcome = reconcile(&t, &base_setpoints, dev_incumbent);
        let limit = enforce_import_limit(&t, &mut outcome.setpoints, limit_incumbent, &no_bounds())
            .unwrap();
        assert!(projected(&t, &outcome.setpoints) <= 1.0);
        dev_incumbent = outcome.active_lever;
        limit_incumbent = limit.active_lever;
        battery_kw = outcome.setpoints["battery"];
        history.push(battery_kw);
    }
    for (i, kw) in history.iter().enumerate().skip(2) {
        assert!(
            (kw - history[1]).abs() < 1e-6,
            "tick {i}: {kw} vs {}",
            history[1]
        );
    }
}

// ── residual accumulator feed ───────────────────────────────────────────────

#[test]
fn residual_kwh_by_asset_counts_limit_adjustments_as_energy() {
    let outcome = ArbiterOutcome {
        absorbed_kwh_by_asset: StdHashMap::from([("battery".to_string(), 0.2)]),
        limit: Some(limit::LimitPassOutcome {
            adjusted_kw_by_asset: StdHashMap::from([
                ("battery".to_string(), -1.8),
                ("heater".to_string(), -1.5),
            ]),
            ..Default::default()
        }),
        ..Default::default()
    };
    let residual = outcome.residual_kwh_by_asset(1.0 / 3600.0);
    assert!((residual["battery"] - (0.2 + 1.8 / 3600.0)).abs() < 1e-12);
    assert!(
        !residual.contains_key("heater"),
        "heater pause feeds no residual"
    );
}

// ── decision trace ──────────────────────────────────────────────────────────

#[test]
fn decision_event_fires_on_a_lever_or_unresolved_change_only() {
    use crate::controller::trace::ControllerEvent;
    let ts = chrono::Utc::now();
    let quiet = (None, 0.0);
    let battery = (Some("battery"), 0.0);
    assert!(decision_event("limit", quiet, quiet, Some(0.9), Some(0.0), ts).is_none());
    assert!(
        decision_event("limit", battery, (Some("battery"), 0.05), None, None, ts).is_none(),
        "a residual within the dead band is not a new decision"
    );
    let engaged = decision_event("limit", quiet, battery, Some(0.9), Some(1.4), ts);
    assert!(matches!(
        engaged,
        Some(ControllerEvent::ArbiterDecision { ref pass, ref active_lever, excess_kw: Some(e), .. })
            if pass == "limit" && active_lever.as_deref() == Some("battery") && e == 1.4
    ));
    assert!(
        decision_event("limit", battery, (Some("battery"), 1.1), None, None, ts).is_some(),
        "an unresolved excess appearing is a decision change"
    );
    assert!(
        decision_event("limit", battery, quiet, None, None, ts).is_some(),
        "released"
    );
}

#[test]
fn capacity_import_limit_at_kw_reads_only_intervals_in_force_now() {
    use crate::entities::capacity::CapacitySnapshot;
    let now = chrono::Utc::now();
    let interval = |from_min: i64, to_min: i64, kw: f64| CapacitySnapshot {
        interval_start: now + chrono::Duration::minutes(from_min),
        interval_end: now + chrono::Duration::minutes(to_min),
        import_limit_kw: Some(kw),
        export_limit_kw: None,
        import_limit_event_id: None,
        export_limit_event_id: None,
    };
    // A stricter limit scheduled for later must not be enforced now.
    let schedule = [interval(-5, 5, 3.0), interval(10, 20, 1.0)];
    assert_eq!(
        limit::capacity_import_limit_at_kw(&schedule, now),
        Some(3.0)
    );
    assert_eq!(
        limit::capacity_import_limit_at_kw(&schedule[1..], now),
        None
    );
    let overlapping = [interval(-5, 5, 3.0), interval(-1, 1, 2.0)];
    assert_eq!(
        limit::capacity_import_limit_at_kw(&overlapping, now),
        Some(2.0)
    );
}
