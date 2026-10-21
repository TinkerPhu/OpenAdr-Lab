//! The opportunistic forecast (openspec `opportunistic-objective` 7.2): the site stepped
//! through the plan's slots with the live tick's decision.

use super::*;
use crate::entities::asset_params::AssetParams;
use crate::entities::design_vocabulary::NonPlanningDispatch;
use chrono::TimeZone;

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 6, 21, 10, 0, 0).unwrap()
}

/// The test profile's assets of the kinds `keep` names.
fn sim_with(keep: impl Fn(&AssetParams) -> bool) -> SimState {
    use rand::SeedableRng;
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/profiles/test.yaml");
    let profile: crate::profile::Profile =
        serde_yaml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let params: Vec<AssetParams> = profile
        .asset_params()
        .into_iter()
        .filter(|p| keep(p))
        .collect();
    SimState::from_params_seeded(&params, t0(), rand::rngs::StdRng::seed_from_u64(7))
}

/// `n` half-hour slots with the PV and base load forecasts given per slot.
fn grid(n: usize, pv_kw: impl Fn(usize) -> f64, base_load_kw: f64) -> ForecastGrid {
    ForecastGrid {
        slot_start: (0..n)
            .map(|t| t0() + chrono::Duration::minutes(30 * t as i64))
            .collect(),
        slot_s: vec![1800.0; n],
        forecast_kw_by_asset: HashMap::from([
            ("pv".to_string(), (0..n).map(pv_kw).collect()),
            ("base_load".to_string(), vec![base_load_kw; n]),
        ]),
    }
}

fn no_requests(_: DateTime<Utc>) -> SlotRequests {
    SlotRequests::default()
}

fn site<'a>(
    schedule: &'a [CapacitySnapshot],
    requests_at: &'a dyn Fn(DateTime<Utc>) -> SlotRequests,
) -> ForecastSite<'a> {
    ForecastSite {
        capacity_schedule: schedule,
        dispatch_windows: &[],
        alert_windows: &[],
        limit_enforcement_enabled: true,
        requests_at,
    }
}

fn soc(trace: &ForecastTrace, id: &str, boundary: usize) -> f64 {
    trace.states_by_asset[id][boundary]["soc"]
}

/// Noon: the PV surplus charges the battery. Evening: the battery covers the base load until
/// it reaches its floor, and the site imports from then on.
#[test]
fn noon_surplus_charges_the_battery_and_the_evening_drains_it_to_its_floor() {
    let sim = sim_with(|p| {
        matches!(
            p,
            AssetParams::Battery(_) | AssetParams::Pv(_) | AssetParams::BaseLoad(_)
        )
    });
    let n = 48;
    let grid = grid(n, |t| if t < 4 { 4.0 } else { 0.0 }, 0.5);
    let trace = opportunistic_forecast(sim, &grid, &site(&[], &no_requests));

    let battery = &trace.net_kw_by_asset["battery"];
    assert!(
        (battery[0] - 3.5).abs() < 0.05,
        "noon: the 3.5 kW surplus charges it, got {}",
        battery[0]
    );
    assert!(
        trace.export_kw[0] < 0.05,
        "nothing left to export, got {}",
        trace.export_kw[0]
    );
    assert!(soc(&trace, "battery", 4) > soc(&trace, "battery", 0));
    assert!(
        (battery[10] + 0.5).abs() < 0.05,
        "evening: it covers the base load, got {}",
        battery[10]
    );
    assert!(trace.import_kw[10] < 0.05);
    assert!(
        (soc(&trace, "battery", n) - 0.10).abs() < 0.02,
        "drained to its floor"
    );
    assert!(battery[n - 1].abs() < 0.05, "at the floor it stops");
    assert!(
        (trace.import_kw[n - 1] - 0.5).abs() < 0.05,
        "and the site imports"
    );
    assert_eq!(
        trace.states_by_asset["battery"].len(),
        n + 1,
        "every step boundary"
    );
}

/// A start-now EV request asks for the charger's full rate, but a scheduled 3 kW import limit
/// holds in every slot after the first, and does not alternate: a charger cut at one step stays
/// cut where its command lands (`limit::hold_where_commands_land`). The first slot draws the
/// command already pending in the charger when the forecast starts (it applies a command one
/// step late); nothing the forecast decides can change that one.
#[test]
fn a_scheduled_import_limit_holds_once_the_first_command_lands() {
    let sim = sim_with(|p| matches!(p, AssetParams::Ev(_) | AssetParams::BaseLoad(_)));
    let pending_kw = match sim.asset("ev").map(|e| &e.state) {
        Some(crate::assets::AssetState::Ev(s)) => s.pending_command_kw,
        other => panic!("the test profile has an EV, got {}", other.is_some()),
    };
    let n = 8;
    let grid = grid(n, |_| 0.0, 0.5);
    let schedule = [CapacitySnapshot {
        interval_start: t0(),
        interval_end: t0() + chrono::Duration::hours(4),
        import_limit_kw: Some(3.0),
        export_limit_kw: None,
        import_limit_event_id: Some("evt-limit".into()),
        export_limit_event_id: None,
        ..Default::default()
    }];
    let start_now = |_: DateTime<Utc>| SlotRequests {
        requests: OpportunisticRequests {
            dispatch_by_asset: HashMap::from([("ev".to_string(), NonPlanningDispatch::StartNow)]),
            ..Default::default()
        },
        ..Default::default()
    };
    let trace = opportunistic_forecast(sim, &grid, &site(&schedule, &start_now));
    assert!(
        (trace.import_kw[0] - (pending_kw + 0.5)).abs() < 1e-9,
        "slot 0 draws the pending command"
    );
    assert!(
        trace.import_kw[1..].iter().all(|&kw| kw <= 3.0 + 1e-9),
        "import {:?}",
        trace.import_kw
    );
    assert!(
        trace.net_kw_by_asset["ev"][1..].iter().all(|&kw| kw > 2.0),
        "the EV charges, limited"
    );
}

/// The forecast feeds PV as a measured reading, which the array takes over its own smoothing:
/// what the forecast says it generates is exactly the plan's series.
#[test]
fn the_forecast_pv_equals_the_input_series() {
    let sim = sim_with(|p| matches!(p, AssetParams::Pv(_) | AssetParams::BaseLoad(_)));
    let series = [0.0, 1.0, 3.0, 4.5, 2.0, 0.2];
    let grid = grid(series.len(), |t| series[t], 0.5);
    let trace = opportunistic_forecast(sim, &grid, &site(&[], &no_requests));
    let pv: Vec<f64> = trace.net_kw_by_asset["pv"].iter().map(|kw| -kw).collect();
    assert_eq!(pv, series.to_vec());
}

/// A BY_DEADLINE EV is planned idle until its latest start and at full power from there: the
/// forecast runs the live tick's decision, so the wait is in the plan too.
#[test]
fn a_waiting_by_deadline_ev_is_planned_idle_and_then_at_full_power() {
    use crate::controller::arbiter::DEADLINE_MARGIN_S;
    let sim = sim_with(|p| matches!(p, AssetParams::Ev(_) | AssetParams::BaseLoad(_)));
    let time_to_target_s = sim.to_sim_snapshot().assets["ev"]
        .full_effort_time_to_target_s
        .expect("the plugged-in EV declares its time to target");
    // The latest start falls on the start of slot 6, three hours in.
    let start_by = t0() + chrono::Duration::hours(3);
    let deadline =
        start_by + chrono::Duration::seconds((time_to_target_s + DEADLINE_MARGIN_S).ceil() as i64);
    let by_deadline = move |_: DateTime<Utc>| SlotRequests {
        requests: OpportunisticRequests {
            dispatch_by_asset: HashMap::from([(
                "ev".to_string(),
                NonPlanningDispatch::StartBy(deadline),
            )]),
            ..Default::default()
        },
        ..Default::default()
    };
    let grid = grid(16, |_| 0.0, 0.5);
    let trace = opportunistic_forecast(sim, &grid, &site(&[], &by_deadline));
    let ev = &trace.net_kw_by_asset["ev"];
    assert!(
        ev[1..6].iter().all(|&kw| kw < 0.1),
        "idle until the start: {ev:?}"
    );
    assert!(
        ev[8..10].iter().all(|&kw| kw > 2.0),
        "full power after it: {ev:?}"
    );
}
