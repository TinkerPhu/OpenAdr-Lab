//! WP4.4 (BL-07): StaleRatePolicy dispatch — slots beyond tariff coverage
//! are filled per the profile-configured policy, flagged `rate_stale`, and
//! surface a stable plan warning (which WP4.3 turns into a notification).

use super::*;
use crate::common::{Interpolation, TimeSeries};
use crate::controller::milp_planner::stale_rates::apply_stale_rate_policy;
use crate::entities::design_vocabulary::StaleRatePolicy;

/// 6 h horizon in 12 × 1800 s slots; policy + percentile injectable.
fn make_profile_6h(policy: StaleRatePolicy, pctl: f64) -> Profile {
    let mut p = make_profile();
    p.planner.plan_step_s = 1800;
    p.planner.plan_horizon_h = 6;
    p.planner.plan_zones = vec![crate::entities::plan::PlanZone {
        step_s: 1800,
        slots: 12,
    }];
    p.planner.stale_rate_policy = policy;
    p.planner.stale_rate_safe_pctl = pctl;
    p
}

/// Import rates covering only the first 2 h of the horizon:
/// 0.40 for the first hour, 0.20 for the next 30 min, 0.10 for the last 30 min.
fn coverage_2h_tariffs() -> TariffTimeSeries {
    let now = fixed_now();
    let snap = |off_min: i64, dur_min: i64, imp: f64| TariffSnapshot {
        interval_start: now + Duration::minutes(off_min),
        interval_end: now + Duration::minutes(off_min + dur_min),
        import_tariff_eur_kwh: Some(imp),
        export_tariff_eur_kwh: Some(0.08),
        co2_g_kwh: Some(300.0),
    };
    TariffTimeSeries::from_snapshots(&[snap(0, 60, 0.40), snap(60, 30, 0.20), snap(90, 30, 0.10)])
}

fn stale_inputs(policy: StaleRatePolicy, pctl: f64) -> MilpInputs {
    let profile = make_profile_6h(policy, pctl);
    let sim = make_snap_from_profile(&profile);
    let tariffs = coverage_2h_tariffs();
    bmi(
        &profile,
        &sim,
        &tariffs,
        &no_capacity(),
        fixed_now(),
        None,
        None,
    )
}

#[test]
fn test_stale_slots_flagged_and_last_known_repeats() {
    let inp = stale_inputs(StaleRatePolicy::LastKnown, 0.5);
    // 2 h coverage on 1800 s slots → slots 0–3 covered, 4–11 stale.
    for t in 0..4 {
        assert!(!inp.rate_stale[t], "slot {t} is covered");
    }
    for t in 4..12 {
        assert!(inp.rate_stale[t], "slot {t} is beyond coverage");
        assert!(
            (inp.c_imp_eur_kwh[t] - 0.10).abs() < 1e-9,
            "LAST_KNOWN repeats the last rate, got {}",
            inp.c_imp_eur_kwh[t]
        );
    }
    let w = inp.stale_rate_warning.as_deref().expect("warning present");
    assert!(w.contains("LAST_KNOWN"), "policy named in warning: {w}");
}

#[test]
fn test_stale_safe_average_uses_percentile_of_known_rates() {
    let inp = stale_inputs(StaleRatePolicy::SafeAverage, 0.5);
    // Known rates {0.10, 0.20, 0.40}; nearest-rank p50 = 0.20.
    for t in 4..12 {
        assert!(
            (inp.c_imp_eur_kwh[t] - 0.20).abs() < 1e-9,
            "SAFE_AVERAGE fills with the percentile rate, got {}",
            inp.c_imp_eur_kwh[t]
        );
    }
}

#[test]
fn test_stale_defer_to_flexible_prices_at_max_known() {
    let inp = stale_inputs(StaleRatePolicy::DeferToFlexible, 0.5);
    // Stale slots priced at the maximum known rate: discretionary load defers
    // into covered slots — the LP analogue of forcing the slots FLEXIBLE.
    for t in 4..12 {
        assert!(
            (inp.c_imp_eur_kwh[t] - 0.40).abs() < 1e-9,
            "DEFER_TO_FLEXIBLE deters allocation in stale slots, got {}",
            inp.c_imp_eur_kwh[t]
        );
    }
}

#[test]
fn test_heuristic_forecast_degrades_to_last_known_with_no_reference_data() {
    // GB-42: with no diurnal_reference (history disabled/empty) and a forward
    // series too narrow to cover any stale slot's 24h-back reference (this
    // fixture's 6h horizon means ref_24 is always well before the series'
    // first sample), HEURISTIC_FORECAST must degrade to LAST_KNOWN — the
    // same guarantee the old stub gave for a fresh VEN with no history.
    let inp = stale_inputs(StaleRatePolicy::HeuristicForecast, 0.5);
    for t in 4..12 {
        assert!(
            (inp.c_imp_eur_kwh[t] - 0.10).abs() < 1e-9,
            "expected LAST_KNOWN degrade (0.10), got {}",
            inp.c_imp_eur_kwh[t]
        );
    }
    let w = inp.stale_rate_warning.as_deref().expect("warning present");
    assert_eq!(
        w,
        "Tariff data ends before the planning horizon; HEURISTIC_FORECAST fills stale slots \
         from a 24h/168h diurnal reference, degrading to LAST_KNOWN where reference data \
         is insufficient",
        "warning text must be stable for WP4.3 notification dedup"
    );
}

/// GB-42: a stale slot's 24h-back reference always falls at/after `now`
/// (since stale slots start at coverage_end), so when the forward series
/// itself is wide enough to already cover that timestamp, no history-store
/// lookup is needed — the answer comes straight from the currently-known
/// series. `fixed_now()` (2026-04-11 06:00 UTC) is a Saturday, so this test
/// uses a Thursday `now` instead to keep the 24h-back reference on the same
/// weekday/weekend bucket as the slot itself (both weekdays).
#[test]
fn test_heuristic_forecast_uses_24h_lookback_within_known_series() {
    let weekday_now = fixed_now() - Duration::days(2); // Thursday 06:00
    let slot_start = weekday_now + Duration::hours(27); // Friday 09:00
    let ref_24 = slot_start - Duration::hours(24); // Thursday 09:00 — same day type
    let series = TimeSeries {
        interpolation: Interpolation::Step,
        samples: vec![(ref_24, 0.42)],
    };
    let slot_bounds = [(slot_start, slot_start + Duration::hours(1))];
    let outcome = apply_stale_rate_policy(
        &StaleRatePolicy::HeuristicForecast,
        0.5,
        &series,
        Some(weekday_now + Duration::hours(1)), // coverage ends well before slot_start → stale
        &slot_bounds,
        0.25,
        "Tariff data",
        None, // no history-store reference needed for the same-day-type path
    );
    assert!(outcome.rate_stale[0], "slot beyond coverage must be stale");
    assert!(
        (outcome.values[0] - 0.42).abs() < 1e-9,
        "expected the forward series' own value at slot_start-24h (0.42), got {}",
        outcome.values[0]
    );
}

/// GB-42: when the 24h-back reference isn't (yet) available in the forward
/// series (even though the day types match), the 168h-back history series is
/// still consulted as a second-chance lookup before degrading to LAST_KNOWN
/// — `diurnal_fill` always checks the history series at its own 168h-back
/// reference, never at the series' 24h-back one.
#[test]
fn test_heuristic_forecast_uses_history_when_beyond_series() {
    let weekday_now = fixed_now() - Duration::days(2); // Thursday 06:00
    let slot_start = weekday_now + Duration::hours(27); // Friday 09:00
    let ref_168 = slot_start - Duration::hours(24 * 7); // previous Friday 09:00
    let series = TimeSeries {
        interpolation: Interpolation::Step,
        samples: vec![(slot_start, 0.10)], // starts only at slot_start — 24h-back not covered
    };
    let diurnal_reference = TimeSeries {
        interpolation: Interpolation::Step,
        samples: vec![(ref_168, 0.33)],
    };
    let slot_bounds = [(slot_start, slot_start + Duration::hours(1))];
    let outcome = apply_stale_rate_policy(
        &StaleRatePolicy::HeuristicForecast,
        0.5,
        &series,
        Some(slot_start), // slot_start itself is the coverage end → stale
        &slot_bounds,
        0.25,
        "Tariff data",
        Some(&diurnal_reference),
    );
    assert!(outcome.rate_stale[0]);
    assert!(
        (outcome.values[0] - 0.33).abs() < 1e-9,
        "expected the history-store second-chance value at ref_168 (0.33), got {}",
        outcome.values[0]
    );
}

/// GB-42: when the 24h-back reference crosses a weekday/weekend boundary
/// relative to the stale slot, the guard must switch to the 168h-back
/// reference (sourced from history) instead of naively using 24h.
#[test]
fn test_heuristic_forecast_weekday_weekend_guard_uses_168h() {
    let now = fixed_now(); // 2026-04-11 06:00 UTC — a Saturday
    let slot_start = now + Duration::hours(3); // Saturday 09:00
    let ref_24 = slot_start - Duration::hours(24); // Friday 09:00 — different day type
    let ref_168 = slot_start - Duration::hours(24 * 7); // the previous Saturday 09:00 — same day type
    use chrono::{Datelike, Weekday};
    let is_weekend = |dt: DateTime<Utc>| matches!(dt.weekday(), Weekday::Sat | Weekday::Sun);
    assert!(
        is_weekend(ref_24) != is_weekend(slot_start),
        "test fixture must actually cross a weekday/weekend boundary at ref_24"
    );
    let series = TimeSeries {
        interpolation: Interpolation::Step,
        samples: vec![(ref_24, 0.11)], // distinguishable "wrong" value the guard must NOT pick
    };
    let diurnal_reference = TimeSeries {
        interpolation: Interpolation::Step,
        samples: vec![(ref_168, 0.77)], // distinguishable "right" value
    };
    let slot_bounds = [(slot_start, slot_start + Duration::hours(1))];
    let outcome = apply_stale_rate_policy(
        &StaleRatePolicy::HeuristicForecast,
        0.5,
        &series,
        Some(slot_start),
        &slot_bounds,
        0.25,
        "Tariff data",
        Some(&diurnal_reference),
    );
    assert!(outcome.rate_stale[0]);
    assert!(
        (outcome.values[0] - 0.77).abs() < 1e-9,
        "day-type guard must use the 168h history value (0.77), not the 24h series value (0.11), got {}",
        outcome.values[0]
    );
}

/// GB-42 regression: the weekday/weekend guard must not be bypassable by
/// falling back to the mismatched-day-type series value when history is
/// unavailable. Even though `series` covers `ref_24` via LOCF (the wrong day
/// type for this slot) and no `diurnal_reference` is supplied, the result
/// must degrade straight to LAST_KNOWN — never silently use Friday's shape
/// for a Saturday slot just because it was the only data on hand. Two
/// distinct series samples are used so `ref_24`'s LOCF value (0.11) and
/// LAST_KNOWN (the most recent sample, 0.99) are provably different numbers.
#[test]
fn test_heuristic_forecast_guard_degrades_to_last_known_not_wrong_day_series_value() {
    let now = fixed_now(); // 2026-04-11 06:00 UTC — a Saturday
    let slot_start = now + Duration::hours(3); // Saturday 09:00
    let ref_24 = slot_start - Duration::hours(24); // Friday 09:00 — different day type
    use chrono::{Datelike, Weekday};
    let is_weekend = |dt: DateTime<Utc>| matches!(dt.weekday(), Weekday::Sat | Weekday::Sun);
    assert!(
        is_weekend(ref_24) != is_weekend(slot_start),
        "test fixture must actually cross a weekday/weekend boundary at ref_24"
    );
    let series = TimeSeries {
        interpolation: Interpolation::Step,
        samples: vec![
            (ref_24 - Duration::hours(2), 0.11), // LOCF value at ref_24 — the "wrong" value
            (ref_24 + Duration::hours(1), 0.99), // most recent sample — LAST_KNOWN
        ],
    };
    let slot_bounds = [(slot_start, slot_start + Duration::hours(1))];
    let outcome = apply_stale_rate_policy(
        &StaleRatePolicy::HeuristicForecast,
        0.5,
        &series,
        Some(slot_start),
        &slot_bounds,
        0.25,
        "Tariff data",
        None, // no history reference available
    );
    assert!(outcome.rate_stale[0]);
    assert!(
        (outcome.values[0] - 0.99).abs() < 1e-9,
        "must degrade to LAST_KNOWN (0.99), not the mismatched-day-type series value (0.11), got {}",
        outcome.values[0]
    );
}

/// BL-07 verify clause: each policy yields different slot costs.
#[test]
fn test_policies_yield_distinguishable_costs() {
    let last = stale_inputs(StaleRatePolicy::LastKnown, 0.5).c_imp_eur_kwh[6];
    let safe = stale_inputs(StaleRatePolicy::SafeAverage, 0.5).c_imp_eur_kwh[6];
    let defer = stale_inputs(StaleRatePolicy::DeferToFlexible, 0.5).c_imp_eur_kwh[6];
    assert!(
        (last - safe).abs() > 1e-9,
        "LAST_KNOWN {last} vs SAFE_AVERAGE {safe}"
    );
    assert!(
        (safe - defer).abs() > 1e-9,
        "SAFE_AVERAGE {safe} vs DEFER {defer}"
    );
    assert!(
        (last - defer).abs() > 1e-9,
        "LAST_KNOWN {last} vs DEFER {defer}"
    );
}

#[test]
fn test_full_coverage_no_stale_no_warning() {
    let profile = make_profile_6h(StaleRatePolicy::HeuristicForecast, 0.5);
    let sim = make_snap_from_profile(&profile);
    let tariffs = make_tariffs(0.25, 0.08, 300.0); // covers now-1h .. now+25h
    let inp = bmi(
        &profile,
        &sim,
        &tariffs,
        &no_capacity(),
        fixed_now(),
        None,
        None,
    );
    assert!(inp.rate_stale.iter().all(|&s| !s), "no slot is stale");
    assert!(
        inp.stale_rate_warning.is_none(),
        "no warning without stale slots"
    );
}

/// BL-17 closeout: CO2 coverage is tracked independently of import-tariff
/// coverage — a VTN that stops sending GHG events well before it stops
/// sending PRICE events (or vice versa) must not conflate the two staleness
/// windows.
#[test]
fn test_co2_coverage_independent_of_import_coverage() {
    let now = fixed_now();
    // Import covers the full 6 h horizon; CO2 covers only the first hour.
    let snap = |off_min: i64, dur_min: i64, imp: f64, co2: Option<f64>| TariffSnapshot {
        interval_start: now + Duration::minutes(off_min),
        interval_end: now + Duration::minutes(off_min + dur_min),
        import_tariff_eur_kwh: Some(imp),
        export_tariff_eur_kwh: Some(0.08),
        co2_g_kwh: co2,
    };
    let tariffs = TariffTimeSeries::from_snapshots(&[
        snap(0, 60, 0.20, Some(300.0)),
        snap(60, 300, 0.20, None), // import continues, GHG events stop
    ]);

    let profile = make_profile_6h(StaleRatePolicy::LastKnown, 0.5);
    let sim = make_snap_from_profile(&profile);
    let inp = bmi(
        &profile,
        &sim,
        &tariffs,
        &no_capacity(),
        fixed_now(),
        None,
        None,
    );

    // Import is fully covered — no import staleness anywhere.
    assert!(
        inp.rate_stale.iter().all(|&s| !s),
        "import tariff covers the whole horizon"
    );
    // CO2 coverage ends after 1 h (slots 0–1 on 1800s slots), stale after.
    assert!(
        inp.co2_stale_rate_warning.is_some(),
        "CO2 coverage ends before the horizon — warning expected"
    );
    assert!(
        inp.co2_stale_rate_warning
            .as_deref()
            .unwrap()
            .contains("GHG"),
        "warning names the GHG data source"
    );
}

#[test]
fn test_rate_estimated_flag_lands_in_plan_slots() {
    let profile = make_profile_6h(StaleRatePolicy::LastKnown, 0.5);
    let mut sim = make_snap_from_profile(&profile);
    set_ev_plugged(&mut sim, false);
    let tariffs = coverage_2h_tariffs();
    let plan = run_planner(
        build_asset_contexts(&profile, &sim, fixed_now(), None, None, &tariffs),
        &sim,
        &tariffs,
        &no_capacity(),
        &profile,
        fixed_now(),
        crate::entities::asset::PlanTrigger::Periodic,
        None,
        None,
        &[],
        None,
        None,
    );
    assert!(!plan.slots[0].rate_estimated, "covered slot not estimated");
    assert!(
        plan.slots[10].rate_estimated,
        "stale slot flagged estimated"
    );
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.message.contains("LAST_KNOWN")),
        "plan carries the stale-rate warning, got {:?}",
        plan.warnings
    );
}

#[test]
fn covered_slot_straddling_tariff_boundary_uses_time_weighted_mean() {
    // R-16 (BL-11): a 10-min slot with 7 min at 0.20 and 3 min at 0.15 must be
    // priced (7*0.20 + 3*0.15)/10 = 0.185, not the slot-start 0.20.
    let now = fixed_now();
    let snap = |off_min: i64, dur_min: i64, imp: f64| TariffSnapshot {
        interval_start: now + Duration::minutes(off_min),
        interval_end: now + Duration::minutes(off_min + dur_min),
        import_tariff_eur_kwh: Some(imp),
        export_tariff_eur_kwh: Some(0.08),
        co2_g_kwh: Some(300.0),
    };
    let tariffs = TariffTimeSeries::from_snapshots(&[snap(0, 7, 0.20), snap(7, 53, 0.15)]);
    let bounds = [(now, now + Duration::minutes(10))];
    let outcome = apply_stale_rate_policy(
        &StaleRatePolicy::LastKnown,
        0.8,
        &tariffs.import_eur_kwh,
        tariffs.import_coverage_end,
        &bounds,
        0.25,
        "Tariff data",
        None,
    );
    assert!(!outcome.rate_stale[0], "slot start is covered");
    assert!(
        (outcome.values[0] - 0.185).abs() < 1e-9,
        "expected blended 0.185, got {}",
        outcome.values[0]
    );
}
