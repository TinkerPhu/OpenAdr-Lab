//! WP4.4 (BL-07): StaleRatePolicy dispatch — slots beyond tariff coverage
//! are filled per the profile-configured policy, flagged `rate_stale`, and
//! surface a stable plan warning (which WP4.3 turns into a notification).

use super::*;
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
fn test_stale_heuristic_forecast_stub_falls_back_to_last_known() {
    // BL-14 (Phase 5) will learn rate patterns; until then HEURISTIC_FORECAST
    // must behave like LAST_KNOWN and say so in the warning.
    let inp = stale_inputs(StaleRatePolicy::HeuristicForecast, 0.5);
    for t in 4..12 {
        assert!((inp.c_imp_eur_kwh[t] - 0.10).abs() < 1e-9);
    }
    let w = inp.stale_rate_warning.as_deref().expect("warning present");
    assert!(
        w.contains("HEURISTIC_FORECAST") && w.contains("LAST_KNOWN"),
        "stub must be explicit: {w}"
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
