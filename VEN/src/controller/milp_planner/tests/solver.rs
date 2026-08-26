use super::*;

// ── Solver tests (run actual HiGHS on synthetic inputs) ──────────────────

/// Build a minimal MilpInputs with no optional assets.
fn make_solver_inputs(n: usize, base_kw: f64) -> MilpInputs {
    MilpInputs {
        n,
        dt_h: vec![1.0; n],
        cum_s: (0..=n as i64).map(|i| i * 3600).collect(),
        c_imp_eur_kwh: vec![0.25; n],
        rate_stale: vec![false; n],
        stale_rate_warning: None,
        co2_stale_rate_warning: None,
        budget_warning: None,
        c_exp_eur_kwh: vec![0.08; n],
        g_imp_kgco2_kwh: vec![0.30; n],
        p_pv_kw: vec![0.0; n],
        p_base_kw: vec![base_kw; n],
        p_imp_max_phys_kw: vec![25.0; n],
        p_exp_max_phys_kw: vec![10.0; n],
        p_imp_max_cont_kw: vec![25.0; n],
        p_exp_max_cont_kw: vec![10.0; n],
        pen_imp_eur_kwh: 0.0,
        pen_exp_eur_kwh: 0.0,
        mip_gap_target: 0.02,
        penalty_rules: vec![],
        e_bat_nom_kwh: None,
        e_bat_init_kwh: None,
        e_bat_min_kwh: None,
        e_bat_max_kwh: None,
        p_bat_ch_max_kw: None,
        p_bat_dis_max_kw: None,
        eff_bat_ch: None,
        eff_bat_dis: None,
        a_ev: vec![false; n],
        ev_mode: MilpLoadMode::MustNotRun,
        t_ev_dead_step: None,
        p_ev_max_kw: 0.0,
        p_ev_min_kw: 0.0,
        e_ev_core_kwh: 0.0,
        e_ev_extra_max_kwh: 0.0,
        v_ev_core_eur: 0.0,
        v_ev_extra_eur_kwh: 0.0,
        heater_mode: MilpLoadMode::MustNotRun,
        t_heat_dead_step: None,
        p_heat_mid_kw: 0.0,
        p_heat_full_kw: 0.0,
        e_heat_init_kwh: 0.0,
        e_heat_max_kwh: 0.0,
        q_heat_dem_kw: 0.0,
        e_heat_target_kwh: 0.0,
        lambda_heat_sw_eur: 0.0,
        w_tier_penalty_eur: 0.0,
        heat_initial_z_mid: 0.0,
        heat_initial_z_full: 0.0,
        shiftable_loads: vec![],
        soc_ev_init: None,
    }
}

fn make_phase1_weights() -> Phase1Weights {
    Phase1Weights {
        w_energy: 1.0,
        w_ghg: 0.0,
        w_grid: 0.0,
        w_import: 0.0,
        w_viol: 1.0,
        c_bat_wear_eur_kwh: 0.0,
        c_bat_ev_coexist_eur_kwh: 0.0,
        c_ctrl_imp_malus_eur_kwh: 0.0,
        w_services: 1.0,
    }
}

fn make_phase2_weights() -> Phase2Weights {
    Phase2Weights {
        c_bat_startup_eur: 0.0,
        c_bat_ramp_eur_kw: 0.0,
        c_ev_startup_eur: 0.0,
        c_ev_ramp_eur_kw: 0.0,
        lambda_heat_sw_eur: 0.0,
        w_tier_penalty_eur: 0.0,
    }
}

/// MayRun EV with v_ev_core_eur exceeding tariff cost → optimizer commits to charging.
#[test]
fn ev_may_run_commits_when_core_reward_exceeds_cost() {
    let mut inputs = make_solver_inputs(4, 0.0);
    inputs.a_ev = vec![true; 4];
    inputs.ev_mode = MilpLoadMode::MayRun;
    inputs.t_ev_dead_step = Some(3);
    inputs.p_ev_max_kw = 7.4;
    inputs.p_ev_min_kw = 0.0;
    inputs.e_ev_core_kwh = 4.0;
    inputs.e_ev_extra_max_kwh = 20.0;
    // tariff = 0.25, cost = 4.0 × 0.25 × 4 slots = up to 4 EUR; reward = 5 EUR > cost
    inputs.v_ev_core_eur = 5.0;

    let result = solve_phase1(
        &inputs,
        &make_phase1_weights(),
        &contexts_from_inputs(&inputs),
        60.0,
    );
    assert!(result.is_ok(), "solver failed: {:?}", result.err());
    let out = result.unwrap();

    let ev_energy: f64 = out
        .p_ev_kw
        .iter()
        .zip(inputs.dt_h.iter())
        .map(|(p, &d)| p * d)
        .sum();
    assert!(
        ev_energy >= inputs.e_ev_core_kwh - 0.1,
        "MayRun EV with sufficient reward should meet core {:.1} kWh, got {:.4}",
        inputs.e_ev_core_kwh,
        ev_energy
    );
}

#[test]
fn solve_feasible_no_optional_assets() {
    // Minimal case: no battery, no EV, no heater. Import exactly covers base load.
    let inputs = make_solver_inputs(4, 0.5); // base = 0.5 kW
    let result = solve_phase1(
        &inputs,
        &make_phase1_weights(),
        &contexts_from_inputs(&inputs),
        60.0,
    );
    assert!(result.is_ok(), "solver failed: {:?}", result.err());
    let out = result.unwrap();
    for t in 0..4 {
        assert!(
            (out.p_imp_kw[t] - 0.5).abs() < 1e-3,
            "p_imp[{t}] = {:.4} should be ≈ 0.5",
            out.p_imp_kw[t]
        );
    }
    assert!(
        out.s_imp_viol_kw.iter().all(|&v| v < 1e-6),
        "unexpected violations"
    );
}

#[test]
fn solve_base_kw_flows_into_net_import() {
    // The baseline load is the sole non-controllable demand term in the power
    // balance — with no controllable asset able to serve it, it must show up
    // one-for-one as grid import.
    let inputs = make_solver_inputs(4, 0.3);
    let result = solve_phase1(
        &inputs,
        &make_phase1_weights(),
        &contexts_from_inputs(&inputs),
        60.0,
    );
    assert!(result.is_ok(), "solver failed: {:?}", result.err());
    let out = result.unwrap();
    for t in 0..4 {
        assert!(
            (out.p_imp_kw[t] - 0.3).abs() < 1e-3,
            "p_imp[{t}] = {:.4} should be ~0.3 (base_kw alone)",
            out.p_imp_kw[t]
        );
    }
}

#[test]
fn solve_ev_must_run_meets_core() {
    // EV MustRun: optimizer must deliver exactly e_ev_core_kwh within deadline.
    let mut inputs = make_solver_inputs(4, 0.0); // no base load
    inputs.a_ev = vec![true; 4];
    inputs.ev_mode = MilpLoadMode::MustRun;
    inputs.t_ev_dead_step = Some(3);
    inputs.p_ev_max_kw = 7.4;
    inputs.p_ev_min_kw = 0.0; // no semi-continuous (cleaner test)
    inputs.e_ev_core_kwh = 4.0;
    inputs.e_ev_extra_max_kwh = 20.0;

    let result = solve_phase1(
        &inputs,
        &make_phase1_weights(),
        &contexts_from_inputs(&inputs),
        60.0,
    );
    assert!(result.is_ok(), "solver failed: {:?}", result.err());
    let out = result.unwrap();

    let ev_energy: f64 = out
        .p_ev_kw
        .iter()
        .zip(inputs.dt_h.iter())
        .map(|(p, &d)| p * d)
        .sum();
    assert!(
        (ev_energy - 4.0).abs() < 1e-2,
        "EV energy {ev_energy:.4} kWh should be ≈ 4.0 kWh"
    );
}

#[test]
fn solve_battery_arbitrage() {
    // Battery should charge at cheap tariff (t=0,1) and discharge at expensive (t=2,3).
    let mut inputs = make_solver_inputs(4, 1.0); // base = 1.0 kW
                                                 // Cheap then expensive tariff
    inputs.c_imp_eur_kwh = vec![0.10, 0.10, 0.30, 0.30];
    // Add battery: init=0, can hold 5 kWh, eff=1
    inputs.e_bat_nom_kwh = Some(5.0);
    inputs.e_bat_init_kwh = Some(0.0);
    inputs.e_bat_min_kwh = Some(0.0);
    inputs.e_bat_max_kwh = Some(5.0);
    inputs.p_bat_ch_max_kw = Some(5.0);
    inputs.p_bat_dis_max_kw = Some(5.0);
    inputs.eff_bat_ch = Some(1.0);
    inputs.eff_bat_dis = Some(1.0);

    let result = solve_phase1(
        &inputs,
        &make_phase1_weights(),
        &contexts_from_inputs(&inputs),
        60.0,
    );
    assert!(result.is_ok(), "solver failed: {:?}", result.err());
    let out = result.unwrap();

    // Both charge patterns are degenerate-optimalat 0.40 EUR. Verify objective value only.
    let obj: f64 = (0..4)
        .map(|t| {
            inputs.c_imp_eur_kwh[t] * out.p_imp_kw[t] * inputs.dt_h[t]
                - inputs.c_exp_eur_kwh[t] * out.p_exp_kw[t] * inputs.dt_h[t]
        })
        .sum();
    assert!(
        (obj - 0.40).abs() < 1e-2,
        "arbitrage objective {obj:.4} EUR should be ≈ 0.40 EUR (charge cheap, discharge expensive)"
    );
    // Battery must discharge in expensive window (at least 1 kWh)
    let dis_in_expensive = out.p_bat_dis_kw[2] + out.p_bat_dis_kw[3];
    assert!(
        dis_in_expensive > 0.5,
        "battery should discharge in expensive period, got {dis_in_expensive:.4} kWh"
    );
}

/// BL-17 closeout: proves `g_imp_kgco2_kwh` × `w_ghg` is load-bearing in the
/// objective, not just parsed-and-ignored (the exact class of bug BL-34's own
/// postmortem warns about — see `docs/reference/KEY_LEARNINGS.md`). Tariff is
/// flat (no price signal at all); only GHG intensity varies. A battery should
/// charge in the clean slots and discharge in the dirty ones purely because
/// `w_ghg > 0` makes that choice cheaper in the objective.
#[test]
fn battery_arbitrage_driven_by_ghg_intensity_alone() {
    let mut inputs = make_solver_inputs(4, 1.0); // base = 1.0 kW
    inputs.c_imp_eur_kwh = vec![0.20; 4]; // flat price — no price signal
    inputs.g_imp_kgco2_kwh = vec![0.10, 0.10, 0.50, 0.50]; // clean then dirty grid
    inputs.e_bat_nom_kwh = Some(5.0);
    inputs.e_bat_init_kwh = Some(0.0);
    inputs.e_bat_min_kwh = Some(0.0);
    inputs.e_bat_max_kwh = Some(5.0);
    inputs.p_bat_ch_max_kw = Some(5.0);
    inputs.p_bat_dis_max_kw = Some(5.0);
    inputs.eff_bat_ch = Some(1.0);
    inputs.eff_bat_dis = Some(1.0);

    let weights = Phase1Weights {
        w_ghg: 5.0, // large enough to dominate the flat price term
        ..make_phase1_weights()
    };

    let result = solve_phase1(&inputs, &weights, &contexts_from_inputs(&inputs), 60.0);
    assert!(result.is_ok(), "solver failed: {:?}", result.err());
    let out = result.unwrap();

    let dis_in_dirty = out.p_bat_dis_kw[2] + out.p_bat_dis_kw[3];
    assert!(
        dis_in_dirty > 0.5,
        "battery should discharge in the high-carbon slots when w_ghg > 0, got {dis_in_dirty:.4}"
    );
    let ch_in_clean = out.p_bat_ch_kw[0] + out.p_bat_ch_kw[1];
    assert!(
        ch_in_clean > 0.5,
        "battery should charge in the low-carbon slots when w_ghg > 0, got {ch_in_clean:.4}"
    );
}

/// BL-17 comfort bidding, the critical test per the BL-34 postmortem lesson in
/// KEY_LEARNINGS.md ("syntactically correct and semantically inert at the same
/// time — verify the variable it rewards is actually load-bearing"): two
/// otherwise-identical Phase 2 solves differ only in the heater's
/// `comfort_full_co2_reward_eur_kwh` (the session's monetized CO2 bid); the
/// reward must measurably shift full-tier usage, not just compile and sit unused.
///
/// `w_tier_penalty_eur = 0.05` gives the baseline (reward = 0.0) a real, if
/// small, reason to avoid full tier — without it, an unrewarded full-tier slot
/// is exactly as cheap in the friction objective as staying off, and the
/// solver's tie-break choice would be undefined rather than deterministic.
#[test]
fn heater_co2_comfort_bid_shapes_phase2_full_tier_usage() {
    use crate::controller::milp_planner::asset_port::{HeaterMilpContext, HeaterMilpMode};
    use crate::services::test_support::milp_mocks::MockHeaterCtx;

    let n = 4;
    let inputs = make_solver_inputs(n, 0.0); // flat 0.25 EUR/kWh import, dt_h=1.0, no load

    let make_ctxs = |co2_reward_eur_kwh: f64| -> Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>> {
        vec![Box::new(MockHeaterCtx {
            ctx: HeaterMilpContext {
                mode: HeaterMilpMode::MayRun,
                t_dead_step: None,
                p_mid_kw: 1.0,
                p_full_kw: 2.0,
                e_init_kwh: 0.0,
                e_max_kwh: 20.0,
                q_dem_kw: 0.0,
                e_target_kwh: 0.0, // no forced target — running is purely reward-driven
                lambda_sw_eur: 0.0,
                initial_z_mid: 0.0,
                initial_z_full: 0.0,
                c_terminal_eur_kwh: 0.0,
                anchored_kw: vec![],
                comfort_full_reward_eur_kwh: 0.0,
                comfort_full_co2_reward_eur_kwh: co2_reward_eur_kwh,
            },
        })]
    };

    let p1w = make_phase1_weights();
    let p2w = Phase2Weights {
        w_tier_penalty_eur: 0.05,
        ..make_phase2_weights()
    };
    // Full tier costs 2.0 kW * 1.0 h * 0.25 EUR/kWh = 0.50 EUR/slot extra import;
    // epsilon must cover running it in every slot (4 * 0.50 = 2.0 EUR) with margin.
    let epsilon = 2.5;

    let out_no_reward = solve_milp_two_phase(&inputs, &p1w, &p2w, epsilon, &make_ctxs(0.0), 60.0)
        .expect("solver failed (no CO2 reward)")
        .0;
    // Reward (0.50) exceeds both the tier penalty (0.05) and the real energy
    // cost (0.50) is covered separately by epsilon — net friction benefit per
    // full-tier slot is unambiguous.
    let out_with_reward =
        solve_milp_two_phase(&inputs, &p1w, &p2w, epsilon, &make_ctxs(0.50), 60.0)
            .expect("solver failed (CO2 reward=0.50)")
            .0;

    let full_slots_no: f64 = out_no_reward.z_heat_full.iter().sum();
    let full_slots_with: f64 = out_with_reward.z_heat_full.iter().sum();
    assert!(
        full_slots_no < 0.5,
        "without a CO2 reward, the tier penalty should keep the heater off full tier: \
         got {full_slots_no}"
    );
    assert!(
        full_slots_with > full_slots_no + 2.0,
        "a CO2 comfort bid, monetized via w_ghg, must be load-bearing in the MILP \
         objective — expected materially more full-tier usage: no_reward={full_slots_no}, \
         with_reward={full_slots_with}"
    );
}

#[test]
fn ev_startup_penalty_produces_contiguous_block() {
    // Flat tariff across 6 slots → degenerate without penalty.
    // With a high startup cost the solver must consolidate EV charging into one run.
    // p_ev_min_kw > 0 makes the semi-continuous constraint bind: z_ev_on=1 forces p_ev >= min,
    // so the solver cannot trivially keep z_ev_on=1 everywhere at zero charging cost.
    let n = 6;
    let mut inputs = make_solver_inputs(n, 0.0);
    inputs.a_ev = vec![true; n];
    inputs.ev_mode = MilpLoadMode::MustRun;
    inputs.t_ev_dead_step = Some(n - 1);
    inputs.p_ev_max_kw = 7.4;
    inputs.p_ev_min_kw = 1.4; // semi-continuous: z_ev_on=1 forces p_ev >= 1.4
    inputs.e_ev_core_kwh = 3.0 * 7.4; // needs 3 full slots at 1 h each

    let mut weights = make_phase2_weights();
    weights.c_ev_startup_eur = 0.5; // high penalty — one startup costs 0.5 EUR

    let out = solve_milp_two_phase(
        &inputs,
        &make_phase1_weights(),
        &weights,
        1.0,
        &contexts_from_inputs(&inputs),
        60.0,
    )
    .expect("solver failed")
    .0;

    // Identify active slots (z_ev_on > 0.5 means EV charging committed)
    let active: Vec<bool> = out.z_ev_on.iter().map(|&v| v > 0.5).collect();
    // Count off→on switches; with startup penalty, expect at most 1 (contiguous block).
    // Starting at slot 0 (0 startups) is also a valid contiguous block.
    let startups = active.windows(2).filter(|w| !w[0] && w[1]).count();
    assert!(
        startups <= 1,
        "expected at most 1 EV startup (contiguous block), got {startups}; active={active:?}"
    );
}

#[test]
fn battery_startup_penalty_minimises_active_restarts() {
    // 6 slots with cheap→expensive tariff pattern.
    // Without penalty: solver may fragment battery into scattered charge/discharge bursts.
    // With high startup penalty: battery should activate in contiguous blocks (≤2 startups:
    // one for charging, one for discharging).
    let n = 6;
    let mut inputs = make_solver_inputs(n, 0.0);
    inputs.c_imp_eur_kwh = vec![0.10, 0.10, 0.10, 0.30, 0.30, 0.30];
    inputs.e_bat_nom_kwh = Some(6.0);
    inputs.e_bat_init_kwh = Some(3.0);
    inputs.e_bat_min_kwh = Some(0.6);
    inputs.e_bat_max_kwh = Some(6.0);
    inputs.p_bat_ch_max_kw = Some(2.0);
    inputs.p_bat_dis_max_kw = Some(2.0);
    inputs.eff_bat_ch = Some(1.0);
    inputs.eff_bat_dis = Some(1.0);

    let mut weights = make_phase2_weights();
    weights.c_bat_startup_eur = 0.5; // high penalty

    let out = solve_milp_two_phase(
        &inputs,
        &make_phase1_weights(),
        &weights,
        1.0,
        &contexts_from_inputs(&inputs),
        60.0,
    )
    .expect("solver failed")
    .0;

    // Count idle→active transitions (mirrors EV startup test logic)
    let active: Vec<bool> = (0..n)
        .map(|t| out.p_bat_ch_kw[t] > 1e-3 || out.p_bat_dis_kw[t] > 1e-3)
        .collect();
    let startups =
        active.windows(2).filter(|w| !w[0] && w[1]).count() + if active[0] { 1 } else { 0 }; // count slot-0 active as a startup
    assert!(
            startups <= 2,
            "expected ≤2 battery startups (charge block + discharge block), got {startups}; active={active:?} ch={:?} dis={:?}",
            out.p_bat_ch_kw, out.p_bat_dis_kw,
        );
}

/// Regression: `pv_use_tiebreak_expr` is mirrored into Phase 2's friction
/// objective (`solver_phase2.rs`) but was missing from `phase1_cap_expr`,
/// the recomputed Phase-1 cost cap. Phase 1's true `c_star` already includes
/// the tiebreak's PV-use bonus; without the same term in the cap expression,
/// even resubmitting Phase 1's own optimal solution through Phase 2 violates
/// `phase1_cap_expr <= c_star + epsilon` by roughly
/// `PV_USE_TIEBREAK_EUR_PER_KWH * total_pv_used_kwh` — which dwarfs the
/// production default epsilon (0.02 €) whenever PV is used at all, making
/// Phase 2 infeasible on every single solve (observed live on Node1).
#[test]
fn phase2_feasible_with_pv_and_production_epsilon() {
    let n = 24;
    let mut inputs = make_solver_inputs(n, 1.0);
    inputs.p_pv_kw = vec![5.0; n]; // substantial PV so the missing bonus >> epsilon
    inputs.e_bat_nom_kwh = Some(10.0);
    inputs.e_bat_init_kwh = Some(5.0);
    inputs.e_bat_min_kwh = Some(1.0);
    inputs.e_bat_max_kwh = Some(10.0);
    inputs.p_bat_ch_max_kw = Some(5.0);
    inputs.p_bat_dis_max_kw = Some(5.0);
    inputs.eff_bat_ch = Some(1.0);
    inputs.eff_bat_dis = Some(1.0);

    let p1w = make_phase1_weights();
    let mut p2w = make_phase2_weights();
    p2w.c_bat_startup_eur = 0.01; // production default

    let contexts = contexts_from_inputs(&inputs);
    let phase1_sol = solve_phase1(&inputs, &p1w, &contexts, 60.0).expect("phase 1 solve failed");
    let c_star = phase1_sol.objective_eur;

    let result = solve_phase2(
        &inputs,
        &p1w,
        &p2w,
        c_star,
        0.02, // production default phase2_epsilon_eur
        &phase1_sol,
        &contexts,
        60.0,
    );

    assert!(
        result.is_ok(),
        "Phase 2 must stay feasible with nontrivial PV usage under the production \
         epsilon budget; got: {:?}",
        result.err()
    );
}

#[test]
fn solve_power_balance_holds() {
    // For every step the power balance constraint must hold in the solution.
    let mut inputs = make_solver_inputs(4, 1.5);
    inputs.p_pv_kw = vec![2.0; 4]; // PV exceeds base, forces export
                                   // Add battery so there are non-trivial flows to check
    inputs.e_bat_nom_kwh = Some(5.0);
    inputs.e_bat_init_kwh = Some(2.5);
    inputs.e_bat_min_kwh = Some(0.5);
    inputs.e_bat_max_kwh = Some(5.0);
    inputs.p_bat_ch_max_kw = Some(3.0);
    inputs.p_bat_dis_max_kw = Some(3.0);
    inputs.eff_bat_ch = Some(1.0);
    inputs.eff_bat_dis = Some(1.0);

    let out = solve_phase1(
        &inputs,
        &make_phase1_weights(),
        &contexts_from_inputs(&inputs),
        60.0,
    )
    .expect("solver failed");

    for t in 0..4 {
        // p_imp + p_pv + p_bat_dis = p_base + p_bat_ch + p_exp (EV=0, heater=0)
        let residual = out.p_imp_kw[t] + inputs.p_pv_kw[t] + out.p_bat_dis_kw[t]
            - inputs.p_base_kw[t]
            - out.p_bat_ch_kw[t]
            - out.p_exp_kw[t];
        assert!(
            residual.abs() < 1e-4,
            "power balance violated at t={t}: residual={residual:.6}"
        );
    }
}

#[test]
fn ev_ramp_penalty_produces_flat_charging_power() {
    // 6 slots, flat tariff → solver is indifferent between e.g. [7.4,1.4,7.4,…] and [4.0,4.0,…].
    // High ramp penalty forces the solver to keep p_ev constant across slots.
    let n = 6;
    let mut inputs = make_solver_inputs(n, 0.0);
    inputs.a_ev = vec![true; n];
    inputs.ev_mode = MilpLoadMode::MustRun;
    inputs.t_ev_dead_step = Some(n - 1);
    inputs.p_ev_max_kw = 7.4;
    inputs.p_ev_min_kw = 1.4;
    inputs.e_ev_core_kwh = 3.0 * 7.4; // needs ~3 full slots at max

    let mut weights = make_phase2_weights();
    weights.c_ev_startup_eur = 0.5; // also penalise startups so EV is one block
    weights.c_ev_ramp_eur_kw = 1.0; // 1 EUR per kW change — very high

    let out = solve_milp_two_phase(
        &inputs,
        &make_phase1_weights(),
        &weights,
        1.0,
        &contexts_from_inputs(&inputs),
        60.0,
    )
    .expect("solver failed")
    .0;

    let active_power: Vec<f64> = out.p_ev_kw.iter().copied().filter(|&v| v > 0.05).collect();
    // All active slots must have the same power (within 0.1 kW rounding)
    if active_power.len() > 1 {
        let first = active_power[0];
        for &p in &active_power[1..] {
            assert!(
                (p - first).abs() < 0.1,
                "EV power varies across active slots: {active_power:?}"
            );
        }
    }
}

#[test]
fn battery_ramp_penalty_produces_smooth_power() {
    // 6 slots, cheap→expensive tariff. Battery should charge in cheap slots, discharge
    // in expensive slots. With high ramp penalty the solver should keep charge and
    // discharge power levels constant across their respective blocks.
    let n = 6;
    let mut inputs = make_solver_inputs(n, 1.0); // 1 kW base load
    inputs.c_imp_eur_kwh = vec![0.08, 0.08, 0.08, 0.30, 0.30, 0.30];
    inputs.e_bat_nom_kwh = Some(6.0);
    inputs.e_bat_init_kwh = Some(3.0);
    inputs.e_bat_min_kwh = Some(0.6);
    inputs.e_bat_max_kwh = Some(6.0);
    inputs.p_bat_ch_max_kw = Some(3.0);
    inputs.p_bat_dis_max_kw = Some(3.0);
    inputs.eff_bat_ch = Some(1.0);
    inputs.eff_bat_dis = Some(1.0);

    let mut weights = make_phase2_weights();
    weights.c_bat_startup_eur = 0.5; // keep blocks contiguous
    weights.c_bat_ramp_eur_kw = 1.0; // very high — force flat power

    let out = solve_milp_two_phase(
        &inputs,
        &make_phase1_weights(),
        &weights,
        1.0,
        &contexts_from_inputs(&inputs),
        60.0,
    )
    .expect("solver failed")
    .0;

    // Check charging slots are at uniform power
    let ch_power: Vec<f64> = out
        .p_bat_ch_kw
        .iter()
        .copied()
        .filter(|&v| v > 0.05)
        .collect();
    if ch_power.len() > 1 {
        let first = ch_power[0];
        for &p in &ch_power[1..] {
            assert!(
                (p - first).abs() < 0.15,
                "battery charge power varies across active slots: {ch_power:?}"
            );
        }
    }
    // Check discharging slots are at uniform power
    let dis_power: Vec<f64> = out
        .p_bat_dis_kw
        .iter()
        .copied()
        .filter(|&v| v > 0.05)
        .collect();
    if dis_power.len() > 1 {
        let first = dis_power[0];
        for &p in &dis_power[1..] {
            assert!(
                (p - first).abs() < 0.15,
                "battery discharge power varies across active slots: {dis_power:?}"
            );
        }
    }
}

#[test]
fn battery_does_not_discharge_during_ev_charging_with_pv_surplus() {
    // 4 slots, flat tariff, PV surplus exceeds ev_min in every slot.
    // Battery has stored energy. EV must charge.
    // High c_bat_ev_coexist → battery should not discharge during EV-on slots.
    let n = 4;
    let mut inputs = make_solver_inputs(n, 0.5);
    inputs.p_pv_kw = vec![5.0; n]; // surplus = 5.0 - 0.5 = 4.5 kW ≥ ev_min

    inputs.e_bat_nom_kwh = Some(10.0);
    inputs.e_bat_init_kwh = Some(8.0);
    inputs.e_bat_min_kwh = Some(0.0);
    inputs.e_bat_max_kwh = Some(10.0);
    inputs.p_bat_ch_max_kw = Some(3.0);
    inputs.p_bat_dis_max_kw = Some(3.0);
    inputs.eff_bat_ch = Some(1.0);
    inputs.eff_bat_dis = Some(1.0);

    inputs.ev_mode = MilpLoadMode::MustRun;
    inputs.a_ev = vec![true; n];
    inputs.t_ev_dead_step = Some(n - 1);
    inputs.p_ev_max_kw = 7.4;
    inputs.p_ev_min_kw = 1.4;
    inputs.e_ev_core_kwh = 4.0 * 1.4; // 5.6 kWh — easily met by PV alone

    let out = solve_phase1(
        &inputs,
        &Phase1Weights {
            c_bat_ev_coexist_eur_kwh: 10.0,
            ..make_phase1_weights()
        },
        &contexts_from_inputs(&inputs),
        60.0,
    )
    .expect("solver failed");

    for t in 0..n {
        if out.z_ev_on[t] > 0.5 {
            assert!(
                out.p_bat_dis_kw[t] < 0.1,
                "slot {t}: battery discharged {:.3} kW while EV charging with PV surplus",
                out.p_bat_dis_kw[t]
            );
        }
    }
}

#[test]
fn ctrl_import_malus_forces_mid_tier_when_pv_covers_mid() {
    // PV surplus = 3 kW exactly matches heater mid tier.
    // With a high malus, importing 3 kW extra for full tier is penalised → planner picks mid.
    let n = 4;
    let mut inputs = make_solver_inputs(n, 2.0); // base = 2 kW
    inputs.p_pv_kw = vec![5.0; n]; // surplus = 5 - 2 = 3 kW
    inputs.c_imp_eur_kwh = vec![0.0; n]; // free energy — removes tariff signal, malus must do the work
    inputs.heater_mode = MilpLoadMode::MayRun;
    inputs.p_heat_mid_kw = 3.0;
    inputs.p_heat_full_kw = 6.0;
    inputs.e_heat_init_kwh = 2.0; // warm tank, not in thermal emergency
    inputs.e_heat_max_kwh = 10.0;
    inputs.e_heat_target_kwh = 10.0;
    inputs.q_heat_dem_kw = 0.1;

    let out = solve_phase1(
        &inputs,
        &Phase1Weights {
            c_ctrl_imp_malus_eur_kwh: 0.25,
            ..make_phase1_weights()
        },
        &contexts_from_inputs(&inputs),
        60.0,
    )
    .expect("solver failed");

    for t in 0..n {
        assert!(
                out.z_heat_full[t] < 0.5,
                "slot {t}: planner chose full tier despite PV exactly covering mid — import malus should prevent this"
            );
        assert!(
            out.p_imp_kw[t] < 0.1,
            "slot {t}: unexpected grid import {:.3} kW with import malus active",
            out.p_imp_kw[t]
        );
    }
}

#[test]
fn ctrl_import_malus_disabled_allows_full_tier() {
    // Mixed tariff: first 2 slots free (c_imp=0, PV surplus=3kW), last 2 slots expensive
    // (c_imp=0.40, no PV). Tank starts cold and needs heating.
    // Without malus the planner should run full tier in free slots to pre-store,
    // avoiding expensive grid heating later.
    let n = 4;
    let mut inputs = make_solver_inputs(n, 2.0);
    inputs.p_pv_kw = vec![5.0, 5.0, 0.0, 0.0]; // surplus=3 kW in first 2 slots only
    inputs.c_imp_eur_kwh = vec![0.0, 0.0, 0.40, 0.40];
    inputs.heater_mode = MilpLoadMode::MustRun;
    inputs.t_heat_dead_step = Some(n - 1);
    inputs.p_heat_mid_kw = 3.0;
    inputs.p_heat_full_kw = 6.0;
    inputs.e_heat_init_kwh = 0.0; // cold tank — must heat
    inputs.e_heat_max_kwh = 10.0;
    inputs.e_heat_target_kwh = 6.0; // reachable in free slots at full tier
    inputs.q_heat_dem_kw = 0.1;

    let out = solve_phase1(
        &inputs,
        &make_phase1_weights(), // c_ctrl_imp_malus_eur_kwh = 0.0
        &contexts_from_inputs(&inputs),
        60.0,
    )
    .expect("solver failed");

    // Without malus, full tier in the free slots is cheaper than paying 0.40 later.
    let used_full = out.z_heat_full.iter().any(|&z| z > 0.5);
    assert!(
        used_full,
        "without malus the planner should pre-store at full tier during free slots; z_full={:?}",
        out.z_heat_full
    );
}

#[test]
fn heater_terminal_reward_raises_end_state() {
    // Flat tariff 0.25 EUR/kWh. Heater in MayRun with empty tank (e_init=0).
    // Without c_terminal: no incentive to heat → tank stays empty.
    // With c_terminal=0.40 > tariff: filling gains 0.15 EUR/kWh → optimizer fills.
    use crate::controller::milp_planner::asset_port::{HeaterMilpContext, HeaterMilpMode};
    use crate::services::test_support::milp_mocks::MockHeaterCtx;

    let n = 4;
    let inputs = make_solver_inputs(n, 0.0); // flat 0.25 EUR/kWh, dt_h=1.0, no load

    let make_ctxs =
        |c_terminal: f64| -> Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>> {
            vec![Box::new(MockHeaterCtx {
                ctx: HeaterMilpContext {
                    mode: HeaterMilpMode::MayRun,
                    t_dead_step: None,
                    p_mid_kw: 1.0,
                    p_full_kw: 2.0,
                    e_init_kwh: 0.0,
                    e_max_kwh: 4.0,
                    q_dem_kw: 0.0,
                    e_target_kwh: 4.0,
                    lambda_sw_eur: 0.0,
                    initial_z_mid: 0.0,
                    initial_z_full: 0.0,
                    c_terminal_eur_kwh: c_terminal,
                    anchored_kw: vec![],
                    comfort_full_reward_eur_kwh: 0.0,
                    comfort_full_co2_reward_eur_kwh: 0.0,
                },
            })]
        };

    let out_no = solve_phase1(&inputs, &make_phase1_weights(), &make_ctxs(0.0), 60.0)
        .expect("solver failed (no c_terminal)");
    let out_yes = solve_phase1(&inputs, &make_phase1_weights(), &make_ctxs(0.40), 60.0)
        .expect("solver failed (c_terminal=0.40)");

    let e_end_no = out_no.e_heat_tank_kwh.last().copied().unwrap_or(0.0);
    let e_end_yes = out_yes.e_heat_tank_kwh.last().copied().unwrap_or(0.0);
    assert!(
        e_end_yes > e_end_no + 0.1,
        "c_terminal=0.40 should fill tank above no-reward baseline; e_end_no={e_end_no:.4} e_end_yes={e_end_yes:.4}"
    );
}

#[test]
fn battery_terminal_reward_raises_end_soc() {
    // Flat tariff 0.25 EUR/kWh. Battery empty (e_init=0), no base load.
    // Without c_terminal: no incentive to charge → battery stays empty.
    // With c_terminal=0.40 > tariff: charging gains 0.15 EUR/kWh → optimizer charges.
    use crate::controller::milp_planner::asset_port::BatteryMilpContext;
    use crate::services::test_support::milp_mocks::MockBatteryCtx;

    let n = 4;
    let inputs = make_solver_inputs(n, 0.0); // flat 0.25 EUR/kWh, dt_h=1.0, no load

    let make_ctxs =
        |c_terminal: f64| -> Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>> {
            vec![Box::new(MockBatteryCtx {
                ctx: BatteryMilpContext {
                    e_nom_kwh: 4.0,
                    e_init_kwh: 0.0,
                    e_min_kwh: 0.0,
                    e_max_kwh: 4.0,
                    p_ch_max_kw: 2.0,
                    p_dis_max_kw: 2.0,
                    eff_ch: 1.0,
                    eff_dis: 1.0,
                    c_terminal_eur_kwh: c_terminal,
                },
            })]
        };

    let out_no = solve_phase1(&inputs, &make_phase1_weights(), &make_ctxs(0.0), 60.0)
        .expect("solver failed (no c_terminal)");
    let out_yes = solve_phase1(&inputs, &make_phase1_weights(), &make_ctxs(0.40), 60.0)
        .expect("solver failed (c_terminal=0.40)");

    // e_bat_kwh has len n+1; index n is the post-horizon end state
    let e_end_no = out_no.e_bat_kwh[n];
    let e_end_yes = out_yes.e_bat_kwh[n];
    assert!(
        e_end_yes > e_end_no + 0.1,
        "c_terminal=0.40 should charge battery above no-reward baseline; e_end_no={e_end_no:.4} e_end_yes={e_end_yes:.4}"
    );
}

#[test]
fn ctrl_import_malus_zero_when_pv_covers_full_tier() {
    // PV surplus > full tier — no import needed regardless. Malus slack = 0, both tiers are free.
    let n = 4;
    let mut inputs = make_solver_inputs(n, 2.0);
    inputs.p_pv_kw = vec![10.0; n]; // surplus = 10 - 2 = 8 kW > full tier (6 kW)
    inputs.c_imp_eur_kwh = vec![0.0; n];
    inputs.heater_mode = MilpLoadMode::MayRun;
    inputs.p_heat_mid_kw = 3.0;
    inputs.p_heat_full_kw = 6.0;
    inputs.e_heat_init_kwh = 0.0;
    inputs.e_heat_max_kwh = 10.0;
    inputs.e_heat_target_kwh = 10.0;
    inputs.q_heat_dem_kw = 0.1;

    let out = solve_phase1(
        &inputs,
        &Phase1Weights {
            c_ctrl_imp_malus_eur_kwh: 0.25,
            ..make_phase1_weights()
        },
        &contexts_from_inputs(&inputs),
        60.0,
    )
    .expect("solver failed");

    // No import should occur (PV covers all), and malus must not block full tier use.
    for t in 0..n {
        assert!(
            out.p_imp_kw[t] < 0.1,
            "slot {t}: unexpected import {:.3} kW when PV covers full tier",
            out.p_imp_kw[t]
        );
    }
}

// ── Marginal-cost dual LP (see docs/architecture/VEN_ARCHITECTURE.md) ────────

#[test]
fn marginal_cost_matches_tariff_when_nothing_binding() {
    // 4 slots, flat tariff, no battery/EV/heater, import cap far above base load —
    // nothing is ever pinned at a bound, so the shadow price on each slot's
    // power-balance row should collapse to the plain per-kWh objective coefficient
    // on p_imp (§5.2 worked example row 1: "nothing else is binding").
    let n = 4;
    let inputs = make_solver_inputs(n, 0.5);
    let contexts = contexts_from_inputs(&inputs);
    let weights = make_phase1_weights();

    let winning = solve_phase1(&inputs, &weights, &contexts, 60.0).expect("phase1 solve failed");
    let marginal = solve_marginal_costs(&inputs, &weights, &contexts, &winning, 60.0)
        .expect("dual LP solve failed");

    assert_eq!(marginal.len(), n);
    for (t, &m) in marginal.iter().enumerate() {
        assert!(
            (m - inputs.c_imp_eur_kwh[t]).abs() < 1e-3,
            "slot {t}: expected marginal cost ≈ tariff {:.3}, got {:.6}",
            inputs.c_imp_eur_kwh[t],
            m
        );
    }
}

#[test]
fn marginal_cost_reflects_binding_import_violation_penalty() {
    // 1 slot: base load (10 kW) exceeds the contractual import cap (2 kW), forcing the
    // solver to use violation slack at cost `pen_imp_eur_kwh`. The extra kWh of import
    // this slot needs comes at tariff + the violation penalty, not tariff alone — the
    // clearest, directly-verifiable case where the marginal cost genuinely differs
    // from the plain tariff (§5.2 worked example row 2's underlying mechanism): the
    // balance row's own variable (p_imp) is party to another active constraint, so
    // its KKT stationarity condition pulls in that constraint's dual too. A battery
    // sitting at its own power bound does *not* by itself move this slot's dual —
    // only a constraint that p_imp itself participates in does.
    let n = 1;
    let mut inputs = make_solver_inputs(n, 10.0);
    inputs.p_imp_max_cont_kw = vec![2.0; n];
    inputs.pen_imp_eur_kwh = 1.0;

    let contexts = contexts_from_inputs(&inputs);
    let weights = make_phase1_weights(); // w_viol = 1.0

    let winning = solve_phase1(&inputs, &weights, &contexts, 60.0).expect("phase1 solve failed");
    assert!(
        winning.s_imp_viol_kw[0] > 1.0,
        "expected the import violation slack to be active, got {:.3}",
        winning.s_imp_viol_kw[0]
    );

    let marginal = solve_marginal_costs(&inputs, &weights, &contexts, &winning, 60.0)
        .expect("dual LP solve failed");

    let expected = inputs.c_imp_eur_kwh[0] + weights.w_viol * inputs.pen_imp_eur_kwh;
    assert!(
        (marginal[0] - expected).abs() < 1e-3,
        "slot 0: expected marginal cost ≈ tariff + violation penalty {:.3}, got {:.6}",
        expected,
        marginal[0]
    );
}

/// The configured `mip_gap_target` must reach both the solver and the produced
/// `Plan`. Before it was configurable this value was the hardcoded
/// `MIP_GAP_TARGET` const, so `Plan.mip_gap_target` — which the UI renders and
/// plan history persists — always read 0.02 no matter what the profile said.
#[test]
fn configured_mip_gap_target_reaches_the_plan() {
    let now = fixed_now();
    let mut profile = make_heater_only_profile(None, 18.0, 23.0, 20.0);
    profile.planner.mip_gap_target = 0.05;
    let sim = make_snap_from_profile(&profile);
    let tariffs = make_tariffs(0.25, 0.08, 300.0);

    let plan = run_planner(
        build_asset_contexts(&profile, &sim, now, None, None, &tariffs),
        &sim,
        &tariffs,
        &no_capacity(),
        &profile,
        now,
        crate::entities::asset::PlanTrigger::Periodic,
        None,
        None,
        &[],
        None,
        None,
    );

    assert_eq!(
        plan.mip_gap_target,
        Some(0.05),
        "profile's mip_gap_target must be persisted on the plan, not the old const"
    );
}
