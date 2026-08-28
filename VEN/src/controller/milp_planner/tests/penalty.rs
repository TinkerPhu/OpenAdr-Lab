use super::*;
use crate::entities::planner_params::PenaltyRuleParams;

// WP6.3 (BL-09) — peak-demand penalty threshold check. Integration tests at the
// `solve_phase1` level, mirroring `tests/solver.rs`'s synthetic-`MilpInputs` style
// (no profile/asset-context machinery needed beyond an EV, which gives us a
// divisible, reschedulable load across slots).

fn base_inputs(n: usize) -> MilpInputs {
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
        p_base_kw: vec![0.0; n],
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
        p_heat_step_kw: 0.0,
        heat_n_stages: 0,
        e_heat_init_kwh: 0.0,
        e_heat_max_kwh: 0.0,
        q_heat_dem_kw: 0.0,
        e_heat_target_kwh: 0.0,
        lambda_heat_sw_eur: 0.0,
        w_tier_penalty_eur: 0.0,
        heat_initial_y: 0.0,
        shiftable_loads: vec![],
        soc_ev_init: None,
    }
}

fn p1w() -> Phase1Weights {
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

fn penalty_rule(threshold_kw: f64, window_s: u64, penalty_eur_per_kw: f64) -> PenaltyRuleParams {
    PenaltyRuleParams {
        rule_id: "peak-10kw".to_string(),
        threshold_kw,
        measurement_window_s: window_s,
        penalty_eur_per_kw,
    }
}

#[test]
fn penalty_rule_disabled_by_default_adds_no_slack_and_matches_unmodified_plan() {
    // Same EV demand as the split test below, but with no penalty rules —
    // must be free to front-load into a single slot with zero penalty cost,
    // and s_penalty_kw must be empty (no rules -> no windows).
    let mut inputs = base_inputs(2);
    inputs.a_ev = vec![true; 2];
    inputs.ev_mode = MilpLoadMode::MustRun;
    inputs.t_ev_dead_step = Some(1);
    inputs.p_ev_max_kw = 12.0;
    inputs.p_ev_min_kw = 0.0;
    inputs.e_ev_core_kwh = 12.0;

    let result = solve_phase1(&inputs, &p1w(), &contexts_from_inputs(&inputs), 60.0);
    assert!(result.is_ok(), "solver failed: {:?}", result.err());
    let out = result.unwrap();
    assert!(
        out.s_penalty_kw.is_empty(),
        "no penalty rules configured -> s_penalty_kw must be empty, got {:?}",
        out.s_penalty_kw
    );
}

#[test]
fn add_penalty_constraints_splits_load_below_threshold() {
    // 12 kWh EV demand over 2 one-hour slots (MustRun, deadline at slot 1),
    // 10 kW threshold with a 1-slot window and a penalty rate high enough
    // that paying it is never cheaper than the (cost-neutral) even split.
    let mut inputs = base_inputs(2);
    inputs.a_ev = vec![true; 2];
    inputs.ev_mode = MilpLoadMode::MustRun;
    inputs.t_ev_dead_step = Some(1);
    inputs.p_ev_max_kw = 12.0;
    inputs.p_ev_min_kw = 0.0;
    inputs.e_ev_core_kwh = 12.0;
    inputs.penalty_rules = vec![penalty_rule(10.0, 3600, 5.0)];

    let result = solve_phase1(&inputs, &p1w(), &contexts_from_inputs(&inputs), 60.0);
    assert!(result.is_ok(), "solver failed: {:?}", result.err());
    let out = result.unwrap();

    for (t, &p) in out.p_imp_kw.iter().enumerate() {
        assert!(
            p <= 10.0 + 1e-3,
            "slot {t} imports {p:.3} kW, exceeds the 10 kW threshold"
        );
    }
    let total_ev_kwh: f64 = out.p_ev_kw.iter().sum();
    assert!(
        (total_ev_kwh - 12.0).abs() < 1e-3,
        "full 12 kWh demand must still be delivered, got {total_ev_kwh:.3}"
    );
    assert_eq!(out.s_penalty_kw.len(), 1, "one configured rule");
    assert!(
        out.s_penalty_kw[0].iter().all(|&s| s < 1e-3),
        "threshold was never breached -> zero penalty slack, got {:?}",
        out.s_penalty_kw[0]
    );
}

#[test]
fn add_penalty_constraints_accepts_penalty_when_reallocation_impossible() {
    // Single one-hour slot, EV MustRun deadline at slot 0 -> no alternative
    // slot exists. 12 kWh in 1 hour forces 12 kW import against a 10 kW
    // threshold; the penalty must be accepted, not silently ignored.
    let mut inputs = base_inputs(1);
    inputs.a_ev = vec![true; 1];
    inputs.ev_mode = MilpLoadMode::MustRun;
    inputs.t_ev_dead_step = Some(0);
    inputs.p_ev_max_kw = 12.0;
    inputs.p_ev_min_kw = 0.0;
    inputs.e_ev_core_kwh = 12.0;
    inputs.penalty_rules = vec![penalty_rule(10.0, 3600, 5.0)];

    let result = solve_phase1(&inputs, &p1w(), &contexts_from_inputs(&inputs), 60.0);
    assert!(result.is_ok(), "solver failed: {:?}", result.err());
    let out = result.unwrap();

    assert!(
        (out.p_imp_kw[0] - 12.0).abs() < 1e-3,
        "core demand must still be met even though it breaches threshold, got {:.3}",
        out.p_imp_kw[0]
    );
    assert_eq!(out.s_penalty_kw.len(), 1);
    assert!(
        out.s_penalty_kw[0][0] > 1.0,
        "expected ~2 kW of penalty slack (12 - 10), got {:?}",
        out.s_penalty_kw[0]
    );
}

#[test]
fn translate_to_plan_emits_warning_and_cost_when_penalty_accepted() {
    let mut inputs = base_inputs(1);
    inputs.a_ev = vec![true; 1];
    inputs.ev_mode = MilpLoadMode::MustRun;
    inputs.t_ev_dead_step = Some(0);
    inputs.p_ev_max_kw = 12.0;
    inputs.e_ev_core_kwh = 12.0;
    inputs.penalty_rules = vec![penalty_rule(10.0, 3600, 5.0)];

    let weights = p1w();
    let contexts = contexts_from_inputs(&inputs);
    let sol = solve_phase1(&inputs, &weights, &contexts, 60.0).expect("solve must succeed");

    let now = chrono::Utc::now();
    let planner = crate::entities::planner_params::PlannerParams {
        penalty_rules: inputs.penalty_rules.clone(),
        ..crate::entities::planner_params::PlannerParams::default()
    };
    let marginal = inputs.c_imp_eur_kwh.clone();
    let plan = translate_to_plan(
        &sol,
        &inputs,
        &weights,
        &planner,
        now,
        crate::entities::asset::PlanTrigger::Periodic,
        None,
        None,
        &[],
        PlannerObjective::MinCost,
        sol.objective_eur,
        0.0,
        None,
        None,
        None,
        &marginal,
    );

    assert!(
        plan.cost_breakdown.c_peak_penalty_eur > 0.0,
        "expected nonzero accepted penalty cost, got {}",
        plan.cost_breakdown.c_peak_penalty_eur
    );
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.message.contains("Penalty rule")),
        "expected a penalty PlanWarning, got: {:?}",
        plan.warnings
    );
    assert_eq!(
        plan.penalty_rules_active.len(),
        1,
        "one configured rule must be reported active"
    );
    assert_eq!(plan.penalty_rules_active[0].rule_id, "peak-10kw");
}
