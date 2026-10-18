//! A plan built from the opportunistic forecast (openspec `opportunistic-objective` 7.3, D8):
//! the trace becomes the plan's flows and states, each asset read back by itself, with the
//! simulated status and the marginal values of D8.

use super::*;
use crate::controller::forecast_trace::{ForecastGrid, ForecastTrace};
use crate::entities::plan::SolveStatus;
use std::collections::HashMap;

/// A request for the n48 profile's battery and PV under the opportunistic objective.
fn request(tariffs: &TariffTimeSeries) -> crate::controller::SolveRequest {
    let profile = make_profile();
    let now = fixed_now();
    let mut contexts = forecast_contexts(&profile, now, Some(4.0));
    contexts.push(Box::new(lossless_battery(10.0, 5.0, 1.0, 5.0)));
    crate::controller::SolveRequest {
        asset_contexts: contexts,
        grid: test_grid(tariffs, &no_capacity()),
        planner: profile.planner.clone(),
        grid_max_import_kw: profile.grid.max_import_kw,
        grid_max_export_kw: profile.grid.max_export_kw,
        now,
        trigger: PlanTrigger::Periodic,
        baseline_override: None,
        objective_override: Some(PlannerObjective::Opportunistic),
        diurnal_import_eur_kwh: None,
        diurnal_co2_g_kwh: None,
    }
}

/// Slot 0 exports, every later slot imports; the battery charges 2 kW in slot 0 from 50 % to
/// 60 %, then idles.
fn trace(grid: &ForecastGrid) -> ForecastTrace {
    let n = grid.slot_start.len();
    let soc = |t: usize| if t == 0 { 0.5 } else { 0.6 };
    ForecastTrace {
        net_kw_by_asset: HashMap::from([(
            "battery".to_string(),
            (0..n).map(|t| if t == 0 { 2.0 } else { 0.0 }).collect(),
        )]),
        states_by_asset: HashMap::from([(
            "battery".to_string(),
            (0..=n)
                .map(|t| HashMap::from([("soc".to_string(), soc(t))]))
                .collect(),
        )]),
        import_kw: (0..n).map(|t| if t == 0 { 0.0 } else { 1.0 }).collect(),
        export_kw: (0..n).map(|t| if t == 0 { 1.5 } else { 0.0 }).collect(),
    }
}

#[test]
fn a_forecast_plan_carries_the_trace_with_the_simulated_status() {
    let tariffs = make_tariffs(0.30, 0.08, 300.0);
    let mut seen = None;
    let plan = plan_from_forecast(request(&tariffs), |grid| {
        seen = Some(grid.clone());
        trace(grid)
    });
    let grid = seen.expect("the forecaster is called once");
    assert_eq!(
        grid.slot_start.len(),
        plan.slots.len(),
        "the forecaster steps the plan's own grid"
    );
    assert_eq!(grid.slot_start[0], fixed_now());
    assert_eq!(
        grid.forecast_kw_by_asset["pv"][0], 4.0,
        "the series the plan is built on"
    );

    assert_eq!(plan.objective, PlannerObjective::Opportunistic);
    assert_eq!(plan.solve_status, SolveStatus::Simulated);
    assert_eq!(plan.friction_eur, 0.0);
    let first = &plan.slots[0];
    assert_eq!(first.net_export_kw, 1.5);
    assert_eq!(first.planned_kw_by_asset["battery"], 2.0);
    assert!((plan.slots[1].planned_state_by_asset["battery"]["soc"] - 0.6).abs() < 1e-12);
}

/// D8: an exporting slot is valued at the export tariff, an importing one at the import tariff.
#[test]
fn a_forecast_slot_is_valued_at_the_tariff_of_the_direction_it_flows() {
    let tariffs = make_tariffs(0.30, 0.08, 300.0);
    let plan = plan_from_forecast(request(&tariffs), trace);
    let (exporting, importing) = (&plan.slots[0], &plan.slots[1]);
    assert_eq!(exporting.marginal_cost_import_eur_per_kwh, 0.08);
    assert_eq!(exporting.marginal_cost_export_eur_per_kwh, 0.08);
    assert_eq!(importing.marginal_cost_import_eur_per_kwh, 0.30);
    assert_eq!(importing.marginal_cost_export_eur_per_kwh, 0.30);
}

/// An overrun the forecast foresees is warned about as a solved plan's would be.
#[test]
fn a_forecast_import_above_the_cap_is_a_capacity_violation() {
    let tariffs = make_tariffs(0.30, 0.08, 300.0);
    let cap_kw = request(&tariffs).grid_max_import_kw;
    let plan = plan_from_forecast(request(&tariffs), |grid| ForecastTrace {
        import_kw: vec![cap_kw + 2.0; grid.slot_start.len()],
        export_kw: vec![0.0; grid.slot_start.len()],
        ..trace(grid)
    });
    assert!(plan
        .warnings
        .iter()
        .any(|w| w.kind == crate::entities::plan::WarningKind::CapacityViolation));
}

/// 7.5: under the opportunistic objective the VTN's `USAGE_FORECAST` report is the forecast:
/// each interval carries the slot's forecast net energy, (import − export) × slot length.
#[test]
fn the_usage_forecast_report_carries_the_forecast_net_power() {
    let tariffs = make_tariffs(0.30, 0.08, 300.0);
    let mut traced = None;
    let plan = plan_from_forecast(request(&tariffs), |grid| {
        let t = trace(grid);
        traced = Some((t.clone(), grid.slot_s.clone()));
        t
    });
    let (trace, slot_s) = traced.unwrap();
    let now = fixed_now();
    let obligation = crate::entities::capacity::OadrReportObligation {
        id: uuid::Uuid::new_v4(),
        event_id: "evt-fleet".into(),
        program_id: Some("prog-fleet".into()),
        payload_type: "USAGE_FORECAST".into(),
        reading_type: "FORECAST".into(),
        resource_name: None,
        due_at: now,
        interval_width_s: 900,
        submit_every_s: 900,
        fulfilled: false,
        created_at: now,
        historical: false,
        aggregate: false,
        ad_hoc: true,
    };
    let report = crate::controller::reporter::build_measurement_report_for_obligation(
        &obligation,
        &HashMap::new(),
        "ven-1",
        None,
        Some(&plan),
        &HashMap::new(),
        None,
        now,
    )
    .expect("an adopted forecast plan is reported");
    let intervals = &report.resources[0].intervals;
    assert_eq!(intervals.len(), plan.slots.len());
    for (t, interval) in intervals.iter().enumerate() {
        let kwh = interval.payloads[0].values[0].as_f64().unwrap();
        let expected_kwh = crate::entities::units::energy_kwh(
            trace.import_kw[t] - trace.export_kw[t],
            crate::entities::units::dt_h_from_s(slot_s[t]),
        );
        assert!(
            (kwh - expected_kwh).abs() < 1e-9,
            "slot {t}: {kwh} kWh, forecast {expected_kwh}"
        );
    }
}
