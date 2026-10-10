//! Golden record of `build_milp_inputs`'s whole output (R-118).
//!
//! `model_fingerprint.rs` pins the MILP model, but its scenarios take only the plain paths of the
//! input builder: no capacity schedule, alert or SIMPLE window, no live or weather forecast, no
//! diurnal reference, no baseline override, no EV budget. This file builds `MilpInputs` for
//! scenarios that between them take every branch and compares the `Debug` text of each with
//! `golden/milp_inputs.txt`, so a refactor of the builder is proven value-identical field by field.
//!
//! The golden was recorded from the builder *before* it was split into stages. Regenerate it only
//! for a deliberate change of the planner's inputs: `UPDATE_INPUTS_GOLDEN=1 cargo test inputs_golden`.

use super::*;
use crate::entities::capacity::{AlertWindow, CapacitySnapshot, SimpleWindow};
use crate::entities::design_vocabulary::{StaleRatePolicy, UserRequestMode};
use crate::entities::device_session::{BaselineOverride, BaselineSlot, EvSession, ShiftableLoad};
use lab_core::time_series::{Interpolation, TimeSeries};

const GOLDEN: &str = include_str!("golden/milp_inputs.txt");

/// Everything one call of the builder takes beyond the profile; `Default` is the plain call.
#[derive(Default)]
struct Case {
    capacity: Option<OadrCapacityState>,
    schedule: Vec<CapacitySnapshot>,
    alerts: Vec<AlertWindow>,
    simple: Vec<SimpleWindow>,
    tariffs: Option<TariffTimeSeries>,
    ev_session: Option<EvSession>,
    shiftables: Vec<ShiftableLoad>,
    baseline_override: Option<BaselineOverride>,
    pv_forecast_override: Option<f64>,
    pv_live_forecast_kw: Option<Vec<f64>>,
    base_load_live_forecast_kw: Option<Vec<f64>>,
    weather_pv_kw: Option<Vec<f64>>,
    diurnal_import_ref: Option<TimeSeries>,
    diurnal_co2_ref: Option<TimeSeries>,
}

fn build(profile: &Profile, case: &Case) -> MilpInputs {
    let now = fixed_now();
    let mut sim = make_snap_from_profile(profile);
    set_ev_plugged(&mut sim, true);
    let tariffs = case
        .tariffs
        .clone()
        .unwrap_or_else(|| make_tariffs(0.25, 0.08, 300.0));
    let capacity = case.capacity.clone().unwrap_or_else(no_capacity);
    let mut ctxs =
        build_asset_contexts(profile, &sim, now, case.ev_session.as_ref(), None, &tariffs);
    push_shiftable_load_contexts(&mut ctxs, &case.shiftables, profile, now);
    super::super::inputs::build_milp_inputs(
        &ctxs,
        &GridSignals {
            capacity_schedule: case.schedule.clone(),
            alert_windows: case.alerts.clone(),
            simple_windows: case.simple.clone(),
            ..test_grid(&tariffs, &capacity)
        },
        &profile.planner,
        &SiteInputs {
            baseline_override: case.baseline_override.as_ref(),
            pv_forecast_override: case.pv_forecast_override,
            pv_live_forecast_kw: case.pv_live_forecast_kw.as_deref(),
            base_load_live_forecast_kw: case.base_load_live_forecast_kw.as_deref(),
            weather_pv_kw: case.weather_pv_kw.as_deref(),
            ..site_inputs(profile)
        },
        &StaleRateRefs {
            import: case.diurnal_import_ref.as_ref(),
            co2: case.diurnal_co2_ref.as_ref(),
        },
        now,
    )
}

fn at(min: i64) -> DateTime<Utc> {
    fixed_now() + Duration::minutes(min)
}

fn simple(level: u8, from_min: i64, to_min: i64) -> SimpleWindow {
    SimpleWindow {
        level,
        start: at(from_min),
        end: at(to_min),
        event_id: format!("simple-{level}"),
    }
}

fn alert(from_min: i64, to_min: i64) -> AlertWindow {
    AlertWindow {
        alert_type: "ALERT_GRID_EMERGENCY".to_string(),
        start: at(from_min),
        end: at(to_min),
        event_id: "alert".to_string(),
        message: String::new(),
    }
}

fn limit(from_min: i64, to_min: i64, imp: Option<f64>, exp: Option<f64>) -> CapacitySnapshot {
    CapacitySnapshot {
        interval_start: at(from_min),
        interval_end: at(to_min),
        import_limit_kw: imp,
        export_limit_kw: exp,
        import_limit_event_id: imp.map(|_| "cap".to_string()),
        export_limit_event_id: exp.map(|_| "cap".to_string()),
    }
}

/// 12 x 1800 s; import and CO2 data cover only the first 2 h (three rates), so 8 slots are stale.
fn stale_setup(policy: StaleRatePolicy) -> (Profile, TariffTimeSeries) {
    let mut p = make_profile();
    p.planner.plan_step_s = 1800;
    p.planner.plan_horizon_h = 6;
    p.planner.plan_zones = vec![crate::entities::plan::PlanZone {
        step_s: 1800,
        slots: 12,
    }];
    p.planner.stale_rate_policy = policy;
    p.planner.stale_rate_safe_pctl = 0.5;
    let snap = |off_min: i64, dur_min: i64, imp: f64, co2: f64| TariffSnapshot {
        interval_start: at(off_min),
        interval_end: at(off_min + dur_min),
        import_tariff_eur_kwh: Some(imp),
        export_tariff_eur_kwh: Some(0.08),
        co2_g_kwh: Some(co2),
    };
    let tariffs = TariffTimeSeries::from_snapshots(&[
        snap(0, 60, 0.40, 420.0),
        snap(60, 30, 0.20, 250.0),
        snap(90, 30, 0.10, 180.0),
    ]);
    (p, tariffs)
}

/// An hourly reference series reaching a week back, so both the 24 h and the 168 h lookback hit.
fn diurnal(base: f64) -> TimeSeries {
    TimeSeries {
        interpolation: Interpolation::Step,
        samples: (-170..=6)
            .map(|h| {
                (
                    fixed_now() + Duration::hours(h),
                    base + 0.01 * (h.rem_euclid(24)) as f64,
                )
            })
            .collect(),
    }
}

fn ev_session(mode: UserRequestMode, budget_eur: Option<f64>) -> EvSession {
    let now = fixed_now();
    crate::entities::device_session::EvSession {
        mode,
        id: uuid::Uuid::from_u128(1),
        budget_eur,
        ..ev_session_until(now, 0.9, now + Duration::hours(2))
    }
}

fn shiftable(n: u128, asset_id: &str, duration_min: u32, latest_end_min: i64) -> ShiftableLoad {
    let now = fixed_now();
    ShiftableLoad {
        id: uuid::Uuid::from_u128(n),
        asset_id: asset_id.to_string(),
        power_kw: 2.0,
        duration_min,
        earliest_start: now,
        latest_end: at(latest_end_min),
        mode: Default::default(),
        created_at: now,
        updated_at: now,
    }
}

fn scenarios() -> Vec<(&'static str, MilpInputs)> {
    let all = make_profile(); // battery, EV, heater, PV, base load; 24 x 300 s
    let mut out = vec![("plain", build(&all, &Case::default()))];

    // Subscription + reservation allowance, and a schedule tighter than it on some slots.
    let capacity = OadrCapacityState {
        import_subscription_kw: Some(12.0),
        import_reservation_kw: Some(3.0),
        export_subscription_kw: Some(6.0),
        ..no_capacity()
    };
    out.push((
        "capacity_allowance_and_schedule",
        build(
            &all,
            &Case {
                capacity: Some(capacity),
                schedule: vec![
                    limit(10, 30, Some(8.0), None),
                    limit(20, 45, Some(20.0), Some(4.0)),
                    limit(60, 90, None, Some(2.0)),
                ],
                ..Default::default()
            },
        ),
    ));

    // SIMPLE 1, 2 and 3 on separate slots, an alert over part of level 1, and two levels overlapping.
    out.push((
        "simple_levels_and_alert",
        build(
            &all,
            &Case {
                simple: vec![
                    simple(1, 0, 20),
                    simple(2, 20, 40),
                    simple(3, 40, 60),
                    simple(1, 35, 45),
                ],
                alerts: vec![alert(10, 15), alert(100, 110)],
                ..Default::default()
            },
        ),
    ));

    for (name, policy) in [
        ("stale_last_known", StaleRatePolicy::LastKnown),
        ("stale_safe_average", StaleRatePolicy::SafeAverage),
        (
            "stale_heuristic_no_reference",
            StaleRatePolicy::HeuristicForecast,
        ),
    ] {
        let (p, tariffs) = stale_setup(policy);
        out.push((
            name,
            build(
                &p,
                &Case {
                    tariffs: Some(tariffs),
                    ..Default::default()
                },
            ),
        ));
    }
    let (p, tariffs) = stale_setup(StaleRatePolicy::HeuristicForecast);
    out.push((
        "stale_heuristic_with_references",
        build(
            &p,
            &Case {
                tariffs: Some(tariffs),
                diurnal_import_ref: Some(diurnal(0.15)),
                diurnal_co2_ref: Some(diurnal(200.0)),
                ..Default::default()
            },
        ),
    ));

    // The PV precedence chain: the pin wins; live is taken as it is; weather is clamped; both run
    // out before the horizon, so later slots fall through to the next source.
    let live: Vec<f64> = (0..10).map(|i| 0.3 * i as f64 - 0.6).collect();
    let weather: Vec<f64> = (0..18).map(|i| 1.5 - 0.2 * i as f64).collect();
    out.push((
        "pv_override_wins",
        build(
            &all,
            &Case {
                pv_forecast_override: Some(2.5),
                pv_live_forecast_kw: Some(live.clone()),
                weather_pv_kw: Some(weather.clone()),
                ..Default::default()
            },
        ),
    ));
    out.push((
        "pv_negative_override_is_zero",
        build(
            &all,
            &Case {
                pv_forecast_override: Some(-1.0),
                ..Default::default()
            },
        ),
    ));
    out.push((
        "pv_live_then_weather_then_model",
        build(
            &all,
            &Case {
                pv_live_forecast_kw: Some(live),
                weather_pv_kw: Some(weather.clone()),
                ..Default::default()
            },
        ),
    ));
    out.push((
        "pv_weather_then_model",
        build(
            &all,
            &Case {
                weather_pv_kw: Some(weather),
                ..Default::default()
            },
        ),
    ));

    // Live base load for the first slots only; the flat profile value fills the rest.
    out.push((
        "base_load_live_then_flat",
        build(
            &all,
            &Case {
                base_load_live_forecast_kw: Some((0..7).map(|i| 0.4 + 0.1 * i as f64).collect()),
                ..Default::default()
            },
        ),
    ));

    // No PV and no base load in the profile: both series are zero.
    let mut bare = make_profile();
    bare.assets
        .retain(|a| !matches!(a, AssetProfile::Pv(_) | AssetProfile::BaseLoad(_)));
    out.push(("no_pv_no_base_load", build(&bare, &Case::default())));

    // Baseline override: two entries on one slot, one before `now` (skipped), one beyond the
    // horizon (lands on the last slot). SIMPLE 2 on the overridden slot caps import at the base
    // load *before* the override.
    let now = fixed_now();
    out.push((
        "baseline_override_after_simple_2",
        build(
            &all,
            &Case {
                baseline_override: Some(BaselineOverride {
                    id: uuid::Uuid::from_u128(9),
                    slots: vec![
                        BaselineSlot {
                            slot_start: at(15),
                            add_kw: 1.0,
                        },
                        BaselineSlot {
                            slot_start: at(17),
                            add_kw: 0.5,
                        },
                        BaselineSlot {
                            slot_start: at(-5),
                            add_kw: 9.0,
                        },
                        BaselineSlot {
                            slot_start: at(600),
                            add_kw: 0.25,
                        },
                    ],
                    created_at: now,
                    updated_at: now,
                }),
                simple: vec![simple(2, 15, 20)],
                ..Default::default()
            },
        ),
    ));

    // EV budget (MAX_COST): one too low to reach the target (warning), one ample (none).
    for (name, budget) in [("ev_budget_too_low", 0.01), ("ev_budget_ample", 500.0)] {
        out.push((
            name,
            build(
                &all,
                &Case {
                    ev_session: Some(ev_session(UserRequestMode::MaxCost, Some(budget))),
                    ..Default::default()
                },
            ),
        ));
    }

    // A deadline session and two shiftable loads beside the profile's own assets.
    out.push((
        "ev_deadline_and_two_shiftables",
        build(
            &all,
            &Case {
                ev_session: Some(ev_session(UserRequestMode::ByDeadline, None)),
                shiftables: vec![shiftable(2, "wm", 30, 65), shiftable(3, "dryer", 45, 110)],
                ..Default::default()
            },
        ),
    ));

    // Two zones of different step: the time grid's running sums.
    let mut zoned = make_profile();
    zoned.planner.plan_zones = vec![
        crate::entities::plan::PlanZone {
            step_s: 300,
            slots: 6,
        },
        crate::entities::plan::PlanZone {
            step_s: 1800,
            slots: 3,
        },
    ];
    out.push(("two_zones", build(&zoned, &Case::default())));

    out
}

fn render() -> String {
    scenarios()
        .into_iter()
        .map(|(name, inputs)| format!("== {name} ==\n{inputs:#?}\n"))
        .collect()
}

fn golden_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/controller/milp_planner/tests/golden/milp_inputs.txt")
}

#[test]
fn inputs_match_the_recorded_golden() {
    let actual = render();
    if std::env::var_os("UPDATE_INPUTS_GOLDEN").is_some() {
        std::fs::write(golden_path(), &actual).expect("write golden");
        eprintln!("inputs golden rewritten: {} lines", actual.lines().count());
        return;
    }
    let golden = GOLDEN.replace("\r\n", "\n");
    for (i, (a, g)) in actual.lines().zip(golden.lines()).enumerate() {
        assert_eq!(
            a,
            g,
            "first difference at line {} of milp_inputs.txt",
            i + 1
        );
    }
    assert_eq!(
        actual.lines().count(),
        golden.lines().count(),
        "same prefix, different length"
    );
}

/// The golden is only worth anything if a run reproduces itself.
#[test]
fn inputs_render_is_reproducible() {
    assert_eq!(render(), render());
}

/// Each scenario must really take the branch it is named for; a golden of the plain path
/// recorded under fourteen names would prove nothing.
#[test]
fn scenarios_take_their_branches() {
    let s: std::collections::HashMap<_, _> = scenarios().into_iter().collect();
    let plain = &s["plain"];
    assert!(s["capacity_allowance_and_schedule"].p_imp_max_cont_kw != plain.p_imp_max_cont_kw);
    assert!(s["capacity_allowance_and_schedule"].p_exp_max_cont_kw != plain.p_exp_max_cont_kw);
    let simple = &s["simple_levels_and_alert"].p_imp_max_cont_kw;
    assert_eq!(simple[2], 0.0, "alert over SIMPLE 1");
    assert!(
        simple[0] > 0.0 && simple[0] < plain.p_imp_max_cont_kw[0],
        "SIMPLE 1"
    );
    assert_eq!(simple[9], 0.0, "SIMPLE 3");
    for name in [
        "stale_last_known",
        "stale_safe_average",
        "stale_heuristic_no_reference",
    ] {
        assert!(s[name].rate_stale.iter().any(|&b| b), "{name}");
        assert!(s[name].stale_rate_warning.is_some(), "{name}");
        assert!(s[name].co2_stale_rate_warning.is_some(), "{name}");
    }
    assert!(
        s["stale_heuristic_with_references"].c_imp_eur_kwh
            != s["stale_heuristic_no_reference"].c_imp_eur_kwh
    );
    assert!(s["pv_override_wins"].p_pv_kw.iter().all(|&kw| kw == 2.5));
    assert!(s["pv_negative_override_is_zero"]
        .p_pv_kw
        .iter()
        .all(|&kw| kw == 0.0));
    let chain = &s["pv_live_then_weather_then_model"].p_pv_kw;
    assert!(chain[0] < 0.0, "live values are not clamped");
    assert_eq!(chain[17], 0.0, "weather values are clamped at zero");
    assert_eq!(
        chain[20], plain.p_pv_kw[20],
        "past both vectors the model answers"
    );
    assert!(s["base_load_live_then_flat"].p_base_kw[3] != plain.p_base_kw[3]);
    assert_eq!(
        s["base_load_live_then_flat"].p_base_kw[10],
        plain.p_base_kw[10]
    );
    assert!(s["no_pv_no_base_load"]
        .p_base_kw
        .iter()
        .all(|&kw| kw == 0.0));
    let over = &s["baseline_override_after_simple_2"];
    assert_eq!(
        over.p_base_kw[3],
        plain.p_base_kw[3] + 1.5,
        "two entries add up"
    );
    assert_eq!(
        over.p_base_kw[23],
        plain.p_base_kw[23] + 0.25,
        "beyond the horizon"
    );
    assert_eq!(
        over.p_base_kw[0], plain.p_base_kw[0],
        "an entry before now is skipped"
    );
    assert_eq!(
        over.p_imp_max_cont_kw[3], plain.p_base_kw[3],
        "SIMPLE 2 caps at the base load before the override"
    );
    assert!(s["ev_budget_too_low"].budget_warning.is_some());
    assert!(s["ev_budget_ample"].budget_warning.is_none());
    assert_eq!(s["ev_deadline_and_two_shiftables"].shiftable_loads.len(), 2);
    assert_eq!(s["two_zones"].n, 9);
    assert_eq!(s["two_zones"].cum_s[9], 6 * 300 + 3 * 1800);
}
