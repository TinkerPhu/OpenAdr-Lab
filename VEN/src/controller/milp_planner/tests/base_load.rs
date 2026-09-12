use super::*;

// ── base-load-competence-consolidation: p_base_kw numeric equivalence ────

/// `build_milp_inputs`'s `p_base_kw` output must match a live `BaseLoad`'s
/// own `forecast_kw_at` directly, not just "tests still pass" — so a future
/// edit to one side can't silently drift from the other (same reasoning as
/// PV's own equivalence tests, `pv.rs`).
#[test]
fn build_milp_inputs_p_base_kw_matches_live_base_load_forecast_kw_at() {
    use crate::assets::BaseLoad;
    use crate::entities::design_vocabulary::AssetHeuristics;
    use crate::simulator::plan_context::resolve_base_load_forecast_kw;
    use crate::simulator::SimState;

    let now = fixed_now(); // 2026-04-11 06:00 UTC, a Saturday
    let profile = make_profile();
    let mut sim_state = SimState::from_params(&profile.assets, now);

    let mut saturday = vec![0.0; 24];
    saturday[6] = 2.0;
    saturday[7] = 5.0;
    let daytime_profile_kw: [Vec<f64>; 7] = std::array::from_fn(|i| {
        if i == 5 {
            saturday.clone()
        } else {
            vec![0.0; 24]
        }
    });
    let heuristic = AssetHeuristics {
        asset_id: "base_load".to_string(),
        daytime_profile_kw,
        seasonal_factor: 1.0,
        last_updated: Some(now),
        recent_mean_abs_error_kw: None,
    };
    {
        let (_, cfg) = sim_state.find_asset_mut("base_load").unwrap();
        let bl = cfg.as_any_mut().downcast_mut::<BaseLoad>().unwrap();
        bl.heuristic = Some(heuristic.clone());
    }
    let bl_ref = {
        let (_, cfg) = sim_state.find_asset("base_load").unwrap();
        cfg.as_any().downcast_ref::<BaseLoad>().unwrap().clone()
    };

    let n_slots: usize = profile.planner.plan_zones.iter().map(|z| z.slots).sum();
    let mut cum_s: Vec<i64> = Vec::with_capacity(n_slots + 1);
    cum_s.push(0);
    for zone in &profile.planner.plan_zones {
        for _ in 0..zone.slots {
            cum_s.push(cum_s.last().unwrap() + zone.step_s as i64);
        }
    }
    let base_load_live_forecast_kw =
        resolve_base_load_forecast_kw(&sim_state, n_slots, &cum_s, now);

    let ctxs: Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>> = vec![];
    let inputs = super::super::inputs::build_milp_inputs(
        &ctxs,
        &TariffTimeSeries::from_snapshots(&[]),
        &no_capacity(),
        &[],
        &[],
        &profile.planner,
        profile.grid.max_import_kw,
        profile.grid.max_export_kw,
        profile.pv_config(),
        profile.assets.iter().find_map(|a| match a {
            AssetProfile::BaseLoad(v) => Some(v),
            _ => None,
        }),
        now,
        None,
        None,
        None,
        base_load_live_forecast_kw.as_deref(),
        None,
        None,
        None,
    );

    for (i, &slot_s) in cum_s[0..n_slots].iter().enumerate() {
        let ts = now + Duration::seconds(slot_s);
        let expected = bl_ref.forecast_kw_at(ts);
        assert!(
            (inputs.p_base_kw[i] - expected).abs() < 1e-9,
            "slot {i}: p_base_kw={}, forecast_kw_at={}",
            inputs.p_base_kw[i],
            expected
        );
    }
    // Sanity: must actually vary (hour 6 -> 2.0, hour 7 -> 5.0) -- otherwise
    // this test wouldn't exercise the heuristic path at all.
    assert!(
        inputs.p_base_kw.contains(&2.0) && inputs.p_base_kw.contains(&5.0),
        "expected both hour-6 (2.0) and hour-7 (5.0) slots, got {:?}",
        inputs.p_base_kw
    );
}
