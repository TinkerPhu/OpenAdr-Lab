//! GB-48: the planner caps each slot by the capacity-limit schedule — a limit
//! announced for later caps only the slots it overlaps.

use super::*;

/// 4 × 30 min slots from `fixed_now()`, grid 25 kW import / 10 kW export.
fn inputs_with(schedule: &[CapacitySnapshot]) -> MilpInputs {
    inputs_with_windows(schedule, &[], &[])
}

fn inputs_with_windows(
    schedule: &[CapacitySnapshot],
    alerts: &[crate::entities::capacity::AlertWindow],
    simple: &[crate::entities::capacity::SimpleWindow],
) -> MilpInputs {
    let now = fixed_now();
    let profile = make_profile_1800s();
    let tariffs = make_tariffs(0.25, 0.08, 300.0);
    super::super::inputs::build_milp_inputs(
        &GridSignals {
            alert_windows: alerts.to_vec(),
            simple_windows: simple.to_vec(),
            ..test_grid(&tariffs, schedule)
        },
        &profile.planner,
        &site_inputs(&profile),
        &NO_REFS,
        now,
        &forecast_contexts(&profile, now, None),
    )
}

/// A schedule segment `[from_min, to_min)` minutes after `fixed_now()`.
fn seg(from_min: i64, to_min: i64, imp: Option<f64>, exp: Option<f64>) -> CapacitySnapshot {
    let now = fixed_now();
    CapacitySnapshot {
        interval_start: now + Duration::minutes(from_min),
        interval_end: now + Duration::minutes(to_min),
        import_limit_kw: imp,
        export_limit_kw: exp,
        import_limit_event_id: imp.map(|_| "cap".to_string()),
        export_limit_event_id: exp.map(|_| "cap".to_string()),
        ..CapacitySnapshot::default()
    }
}

// Moved from `basic.rs::capacity_event_overrides_grid_limit`: same intent (an
// OpenADR limit caps the contractual bound, the physical bound stays), but the
// limit arrives as a schedule segment.
#[test]
fn capacity_limit_covering_the_horizon_caps_every_slot_physical_unchanged() {
    let inputs = inputs_with(&[seg(-60, 180, Some(5.0), None)]);
    assert!(inputs
        .p_imp_max_phys_kw
        .iter()
        .all(|&v| (v - 25.0).abs() < 1e-9));
    assert!(inputs
        .p_imp_max_cont_kw
        .iter()
        .all(|&v| (v - 5.0).abs() < 1e-9));
}

#[test]
fn limit_announced_in_advance_caps_only_its_slots() {
    // A 0 kW limit for the third half hour: slots 0, 1 and 3 keep the grid's 25 kW.
    let inputs = inputs_with(&[seg(60, 90, Some(0.0), None)]);
    assert_eq!(inputs.p_imp_max_cont_kw, vec![25.0, 25.0, 0.0, 25.0]);
}

#[test]
fn consecutive_limits_cap_their_own_slots() {
    // The seed's ev-charge-pause shape: 0 kW then 7.4 kW.
    let inputs = inputs_with(&[seg(30, 60, Some(0.0), None), seg(60, 90, Some(7.4), None)]);
    assert_eq!(inputs.p_imp_max_cont_kw, vec![25.0, 0.0, 7.4, 25.0]);
}

#[test]
fn a_short_limit_inside_a_slot_caps_the_whole_slot() {
    // 10 minutes of 1.5 kW in the middle of slot 1: never plan through it.
    let inputs = inputs_with(&[seg(40, 50, Some(1.5), None)]);
    assert_eq!(inputs.p_imp_max_cont_kw, vec![25.0, 1.5, 25.0, 25.0]);
}

#[test]
fn export_limits_cap_their_own_slots() {
    let inputs = inputs_with(&[seg(0, 30, None, Some(0.5))]);
    assert_eq!(inputs.p_exp_max_cont_kw, vec![0.5, 10.0, 10.0, 10.0]);
    assert!(inputs.p_imp_max_cont_kw.iter().all(|&v| v == 25.0));
}

/// An allowance segment `[from_min, to_min)` minutes after `fixed_now()`.
fn allowance_seg(
    from_min: i64,
    to_min: i64,
    subscription_kw: Option<f64>,
    reservation_kw: Option<f64>,
) -> CapacitySnapshot {
    let now = fixed_now();
    CapacitySnapshot {
        interval_start: now + Duration::minutes(from_min),
        interval_end: now + Duration::minutes(to_min),
        import_subscription_kw: subscription_kw,
        import_reservation_kw: reservation_kw,
        ..CapacitySnapshot::default()
    }
}

#[test]
fn allowance_still_combines_with_the_slot_limit() {
    let inputs = inputs_with(&[
        seg(0, 30, Some(1.0), None),
        allowance_seg(-60, 180, None, Some(3.0)),
    ]);
    assert_eq!(inputs.p_imp_max_cont_kw, vec![1.0, 3.0, 3.0, 3.0]);
}

// GB-48 C: subscription and reservation are schedules, so a reservation binds only
// the slots it covers.
#[test]
fn a_future_reservation_raises_only_its_own_slots() {
    let inputs = inputs_with(&[
        allowance_seg(-60, 60, Some(5.0), None),
        allowance_seg(60, 120, Some(5.0), Some(3.0)),
        allowance_seg(120, 180, Some(5.0), None),
    ]);
    assert_eq!(inputs.p_imp_max_cont_kw, vec![5.0, 5.0, 8.0, 8.0]);
}

// User Guide §8.10.1: once the limit schedule is exhausted the subscription is the
// default limit. No dedicated branch: the allowance caps every slot no limit covers.
#[test]
fn the_subscription_takes_over_once_the_limit_schedule_is_exhausted() {
    let inputs = inputs_with(&[
        seg(0, 60, Some(2.0), None),
        allowance_seg(-60, 180, Some(4.0), None),
    ]);
    assert_eq!(inputs.p_imp_max_cont_kw, vec![2.0, 2.0, 4.0, 4.0]);
}

#[test]
fn an_exhausted_limit_schedule_without_a_subscription_leaves_the_physical_bound() {
    let inputs = inputs_with(&[seg(0, 60, Some(2.0), None)]);
    assert_eq!(inputs.p_imp_max_cont_kw, vec![2.0, 2.0, 25.0, 25.0]);
}

#[test]
fn simple_level_1_takes_its_fraction_of_the_slot_cap() {
    let now = fixed_now();
    let level1 = crate::entities::capacity::SimpleWindow {
        level: 1,
        start: now,
        end: now + Duration::hours(2),
        event_id: "simple".to_string(),
    };
    let inputs = inputs_with_windows(&[seg(30, 60, Some(4.0), None)], &[], &[level1]);
    // Default 50 %: 12.5 kW of the grid's 25, 2.0 kW of the slot's 4.0 kW limit.
    assert_eq!(inputs.p_imp_max_cont_kw, vec![12.5, 2.0, 12.5, 12.5]);
}

#[test]
fn an_alert_still_zeroes_its_slots_over_a_limit() {
    let now = fixed_now();
    let alert = crate::entities::capacity::AlertWindow {
        alert_type: "ALERT_GRID_EMERGENCY".to_string(),
        start: now,
        end: now + Duration::minutes(30),
        event_id: "alert".to_string(),
        message: String::new(),
    };
    let inputs = inputs_with_windows(&[seg(0, 60, Some(4.0), None)], &[alert], &[]);
    assert_eq!(inputs.p_imp_max_cont_kw, vec![0.0, 4.0, 25.0, 25.0]);
}
