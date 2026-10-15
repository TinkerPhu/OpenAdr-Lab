//! Several assets of one kind in one plan. The planner core knows no asset kind, so two batteries
//! or two EVs are planned like any two assets: each with its own variables, costs, readback and
//! plan result, by its own id. Most tests hand the planner its contexts directly; the last plans
//! the multi-asset test profile end to end, through the production context builder (W7).

use super::*;
use crate::controller::milp_interactions::{
    AssetInteraction, BatEvCoexistInteraction, GlobalMilpInputs, GridMilpVars, InteractionVars,
    MilpVarPool,
};
use crate::controller::milp_planner::DeclareMode;
use crate::entities::device_session::{EvSession, EvSessionOrigin};
use good_lp::{variable, ProblemVariables};

/// A battery of `capacity_kwh` at `soc_frac`, 10 % minimum, `p_max_kw` both ways, planned as
/// `id`.
fn battery(id: &str, capacity_kwh: f64, soc_frac: f64, p_max_kw: f64) -> BatteryMilpContext {
    BatteryMilpContext {
        asset_id: id.into(),
        ..battery_ctx(
            capacity_kwh,
            soc_frac * capacity_kwh,
            0.1 * capacity_kwh,
            p_max_kw,
            0.95,
        )
    }
}

/// A 60 kWh, 7.4 kW EV plugged at 30 %, planned as `id`, serving `sessions` over `profile`'s
/// grid.
fn ev(id: &str, sessions: &[EvSession], profile: &Profile, now: DateTime<Utc>) -> EvMilpContext {
    let crate::entities::time_grid::TimeGrid { n, cum_s, .. } =
        crate::entities::time_grid::TimeGrid::from_zones(&profile.planner.plan_zones);
    let charger = EvCharger::from_params(&EvParams {
        id: id.into(),
        battery_kwh: 60.0,
        max_charge_kw: 7.4,
        min_charge_kw: 1.4,
        initial_soc_frac: 0.30,
        soc_target_frac: 0.8,
        ..Default::default()
    });
    let state = AssetState::Ev(EvState {
        soc_frac: 0.30,
        actual_power_kw: 0.0,
        plugged: true,
        pending_command_kw: 0.0,
        was_away_by_usage_sim: false,
    });
    EvMilpContext::from_state(
        id,
        &state,
        &charger,
        n,
        &cum_s,
        now,
        sessions,
        &[],
        1.4,
        0.0,
        1.0,
        0.0,
        0.0,
        0.0,
    )
}

fn session(id: u128, target_soc_frac: f64, departure: DateTime<Utc>) -> EvSession {
    let now = fixed_now();
    EvSession {
        id: uuid::Uuid::from_u128(id),
        origin: EvSessionOrigin::UserRequest,
        ..ev_session_until(now, target_soc_frac, departure)
    }
}

/// `ctxs` planned over `profile` at flat tariffs (0.25 / 0.08 EUR/kWh) with no capacity limit.
fn plan_flat(
    ctxs: Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>>,
    profile: &Profile,
    now: DateTime<Utc>,
) -> Plan {
    run_planner(
        ctxs,
        &make_tariffs(0.25, 0.08, 300.0),
        &no_capacity(),
        profile,
        now,
        PlanTrigger::Periodic,
        None,
        None,
    )
}

/// Each asset's allocated power per slot [kW], 0.0 where it has no allocation.
fn allocated_kw(plan: &Plan, asset_id: &str) -> Vec<f64> {
    plan.slots
        .iter()
        .map(|s| {
            s.allocations
                .iter()
                .filter(|a| a.asset_id == asset_id)
                .map(|a| a.power_kw)
                .sum()
        })
        .collect()
}

#[test]
fn two_batteries_are_planned_independently() {
    let now = fixed_now();
    // 4 × 30 min. A wide spread, no controllable-import malus and a 6 kW base load that
    // neither battery can cover alone, so storing cheap energy for the dear hour pays for both.
    let mut profile = base_load_only_profile();
    profile.planner.c_ctrl_imp_malus_eur_kwh = 0.0;
    if let Some(AssetParams::BaseLoad(base)) = profile.assets.first_mut() {
        base.baseline_kw = 6.0;
    }
    let tariffs = make_two_zone_tariffs(0.05, 0.50);
    let ctxs: Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>> = vec![
        Box::new(battery("bat-a", 10.0, 0.2, 5.0)),
        Box::new(battery("bat-b", 5.0, 0.9, 3.0)),
    ];
    let plan = run_planner(
        ctxs,
        &tariffs,
        &no_capacity(),
        &profile,
        now,
        PlanTrigger::Periodic,
        None,
        None,
    );
    assert_eq!(
        plan.solve_status,
        crate::entities::plan::SolveStatus::Optimal
    );

    // Each battery starts from its own state and stays inside its own bounds.
    let soc = |t: usize, id: &str| plan.slots[t].planned_state_by_asset[id]["soc"];
    assert!((soc(0, "bat-a") - 0.2).abs() < 1e-9);
    assert!((soc(0, "bat-b") - 0.9).abs() < 1e-9);
    for t in 0..plan.slots.len() {
        for id in ["bat-a", "bat-b"] {
            assert!(
                (0.1 - 1e-6..=1.0 + 1e-6).contains(&soc(t, id)),
                "slot {t}: {id} at {} leaves its own bounds",
                soc(t, id)
            );
        }
    }
    // Each one has its own allocations; neither overwrote the other.
    for id in ["bat-a", "bat-b"] {
        assert!(
            allocated_kw(&plan, id).iter().any(|kw| kw.abs() > 0.01),
            "{id} must be planned on its own, got {:?}",
            plan.slots
                .iter()
                .map(|s| &s.allocations)
                .collect::<Vec<_>>()
        );
    }
    // The plan names both, and the site balances over both of them.
    assert!(["bat-a", "bat-b"]
        .iter()
        .all(|id| plan.planned_asset_ids.iter().any(|p| p == id)));
    assert_site_balance(&plan, 0.05);
}

#[test]
fn two_evs_keep_their_own_obligations() {
    let now = fixed_now();
    let profile = base_load_only_profile();
    let tariffs = make_tariffs(0.25, 0.08, 300.0);
    // car-a: 6 kWh by the end of the horizon, easily reachable. car-b: 30 kWh in half an
    // hour, which no 7.4 kW charger can deliver.
    let reachable = session(11, 0.40, now + Duration::hours(2));
    let unreachable = session(22, 0.80, now + Duration::minutes(30));
    let ctxs: Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>> = vec![
        Box::new(ev("car-a", std::slice::from_ref(&reachable), &profile, now)),
        Box::new(ev(
            "car-b",
            std::slice::from_ref(&unreachable),
            &profile,
            now,
        )),
    ];
    let plan = run_planner(
        ctxs,
        &tariffs,
        &no_capacity(),
        &profile,
        now,
        PlanTrigger::Periodic,
        None,
        None,
    );

    let delivered_kwh =
        |id: &str| -> f64 { allocated_kw(&plan, id).iter().map(|kw| kw * 0.5).sum() };
    assert!(
        delivered_kwh("car-a") >= 6.0 - 0.05,
        "car-a charges toward its own 6 kWh, got {:.2} kWh",
        delivered_kwh("car-a")
    );
    assert!(
        delivered_kwh("car-b") > 3.0,
        "car-b charges what its window allows, got {:.2} kWh",
        delivered_kwh("car-b")
    );

    // The shortfall names the session that falls short, and only that one.
    let shortfalls: Vec<&str> = plan
        .warnings
        .iter()
        .filter(|w| w.kind == crate::entities::plan::WarningKind::EvCoreEnergyUnmet)
        .map(|w| w.message.as_str())
        .collect();
    assert_eq!(shortfalls.len(), 1, "one EV falls short: {shortfalls:?}");
    assert!(shortfalls[0].contains(&unreachable.id.to_string()));
    assert!(!shortfalls[0].contains(&reachable.id.to_string()));

    // Each EV reports its own session's window.
    let window_end = |id: &str| {
        plan.session_flexibility
            .iter()
            .find(|f| f.asset_id == id)
            .map(|f| f.window_end)
    };
    assert_eq!(window_end("car-a"), Some(reachable.departure_time));
    assert_eq!(window_end("car-b"), Some(unreachable.departure_time));
}

/// The grid variables of one solve, for a pool built by hand.
fn grid_vars(vars: &mut ProblemVariables, n: usize) -> GridMilpVars {
    let mut series =
        || -> Vec<good_lp::Variable> { (0..n).map(|_| vars.add(variable().min(0.0))).collect() };
    GridMilpVars {
        p_imp: series(),
        p_exp: series(),
        u_grid: series(),
        s_imp_viol: series(),
        s_exp_viol: series(),
    }
}

/// How many (discharger, switched load) pairs the coexistence penalty couples among `ctxs`.
fn coexist_pairs(ctxs: &[&dyn crate::controller::milp_planner::AssetMilpContext]) -> Option<usize> {
    let n = 4;
    let mut vars = good_lp::variables!();
    let pool = MilpVarPool {
        grid: grid_vars(&mut vars, n),
        assets: ctxs
            .iter()
            .map(|c| c.declare(n, DeclareMode::Plan { friction: false }, &mut vars))
            .collect(),
    };
    // PV surplus covers the EV's minimum power in every slot.
    let global = GlobalMilpInputs {
        n,
        dt_h: vec![0.5; n],
        forecast_surplus_kw: vec![6.0 - 1.0; n],
    };
    let interaction = BatEvCoexistInteraction { c_eur_kwh: 0.05 };
    if !interaction.applicable(&pool) {
        return None;
    }
    match interaction.declare_vars(&pool, &global, &mut vars) {
        InteractionVars::BatEvCoexist { pairs } => Some(pairs.len()),
        InteractionVars::CtrlImportMalus { .. } => unreachable!(),
    }
}

#[test]
fn coexistence_couples_every_discharger_with_every_switched_load() {
    let now = fixed_now();
    let profile = base_load_only_profile();
    let bat_a = battery("bat-a", 10.0, 0.5, 5.0);
    let bat_b = battery("bat-b", 5.0, 0.5, 3.0);
    let car = ev("car", &[], &profile, now);
    assert_eq!(coexist_pairs(&[&bat_a, &bat_b, &car]), Some(2));
    assert_eq!(coexist_pairs(&[&bat_a, &car]), Some(1));
    // No discharger: the interaction does not apply and declares nothing.
    assert_eq!(coexist_pairs(&[&car]), None);
}

/// A PV array forecasting `forecast_kw` in every slot, planned as `id`.
fn pv_array(
    id: &str,
    forecast_kw: f64,
    n: usize,
) -> Box<dyn crate::controller::milp_planner::AssetMilpContext> {
    Box::new(crate::controller::milp_planner::ForecastMilpContext {
        asset_id: id.into(),
        role: crate::controller::milp_planner::ForecastRole::Generation,
        forecast_kw: vec![forecast_kw; n],
        curtailable: true,
    })
}

/// W3: two PV arrays take part side by side. Under an export cap below their surplus the plan
/// curtails; the site's PV forecast is their sum, and neither array is dispatched: no allocation,
/// no planned power, not among the planned assets.
#[test]
fn two_pv_arrays_are_planned_as_one_site_forecast() {
    let now = fixed_now();
    let mut profile = base_load_only_profile(); // 1 kW base load, 4 × 30 min
    profile.grid.max_export_kw = 1.0;
    let n = crate::entities::time_grid::slot_count(&profile.planner.plan_zones);
    let mut ctxs = forecast_contexts(&profile, now, None);
    ctxs.insert(0, pv_array("pv-roof", 3.0, n));
    ctxs.insert(1, pv_array("pv-garage", 2.0, n));
    let plan = plan_flat(ctxs, &profile, now);
    assert_eq!(
        plan.solve_status,
        crate::entities::plan::SolveStatus::Optimal
    );
    assert!(
        plan.planned_asset_ids.is_empty(),
        "{:?}",
        plan.planned_asset_ids
    );
    for slot in &plan.slots {
        assert!(
            (slot.pv_forecast_kw - 5.0).abs() < 1e-9,
            "the sum of both arrays"
        );
        assert!((slot.baseline_kw - 1.0).abs() < 1e-9);
        // 5 kW generated, 1 kW used on site, at most 1 kW exported: the rest is curtailed.
        assert!(
            slot.pv_used_kw <= 2.0 + 1e-6 && slot.pv_used_kw >= 1.0 - 1e-6,
            "slot {}: used {:.3} kW",
            slot.slot_index,
            slot.pv_used_kw
        );
        assert!(slot.allocations.is_empty() && slot.planned_kw_by_asset.is_empty());
    }
}

/// W7: the multi-asset test profile (two EVs, an east and a west array) validates, and the
/// production context builder and the planner plan each asset under its own id: both EVs are
/// planned assets with their own session, both arrays forecast assets with their own power.
#[test]
fn the_multi_asset_profile_plans_every_asset_under_its_own_id() {
    let now = fixed_now();
    let profile = fleet_profile("multi_asset_test");
    let mut sim = crate::simulator::SimState::from_params(&profile.assets, now);
    for id in ["car-a", "car-b"] {
        if let Some((entry, _)) = sim.find_asset_mut(id) {
            if let AssetState::Ev(s) = &mut entry.state {
                s.plugged = true;
            }
        }
    }
    let crate::entities::time_grid::TimeGrid { n, cum_s, .. } =
        crate::entities::time_grid::TimeGrid::from_zones(&profile.planner.plan_zones);
    let mut queue = crate::entities::device_session::EvSessionQueue::default();
    queue
        .insert(EvSession {
            asset_id: "car-b".into(),
            ..session(7, 0.6, now + Duration::hours(6))
        })
        .expect("one session");
    let sessions = std::collections::BTreeMap::from([("car-b".to_string(), queue)]);
    let ctxs = crate::simulator::plan_context::build_asset_contexts(
        &sim,
        n,
        &cum_s,
        now,
        &sessions,
        &std::collections::BTreeMap::new(),
        &profile.planner,
        &std::collections::HashMap::new(),
        &std::collections::BTreeMap::new(),
        &std::collections::HashMap::new(),
        None,
    );
    let plan = plan_flat(ctxs, &profile, now);
    assert!(plan.solve_status.solved(), "{:?}", plan.solve_status);
    for id in ["car-a", "car-b", "battery"] {
        assert!(
            plan.planned_asset_ids.iter().any(|p| p == id),
            "{id} is planned"
        );
    }
    for id in ["pv-east", "pv-west", "base_load"] {
        assert!(
            plan.slots[0].forecast_by_asset.contains_key(id),
            "{id} is forecast"
        );
    }
    // car-b reaches its session's target by its departure; car-a has none to serve.
    let delivered_kwh: f64 = allocated_kw(&plan, "car-b")
        .iter()
        .zip(&plan.slots)
        .map(|(kw, s)| kw * (s.end - s.start).num_seconds() as f64 / 3600.0)
        .sum();
    assert!(delivered_kwh > 0.0, "car-b charges for its own session");
}
