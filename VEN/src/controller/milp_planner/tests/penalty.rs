use super::*;
use crate::entities::planner_params::PenaltyRuleParams;

// WP6.3 (BL-09) — peak-demand penalty threshold check. Integration tests at the
// `solve_phase1` level, mirroring `tests/solver.rs`'s synthetic-`MilpInputs` style
// (no profile/asset-context machinery needed beyond an EV, which gives us a
// divisible, reschedulable load across slots).

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
    let mut inputs = make_solver_inputs(2, 0.0);
    inputs.a_ev = vec![true; 2];
    inputs.ev_mode = MilpLoadMode::MustRun;

    inputs.p_ev_max_kw = 12.0;
    inputs.p_ev_min_kw = 0.0;
    set_ev_firm(&mut inputs, 12.0, 1);

    let result = solve_phase1(
        &inputs,
        &make_phase1_weights(),
        &contexts_from_inputs(&inputs),
        60.0,
    );
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
    let mut inputs = make_solver_inputs(2, 0.0);
    inputs.a_ev = vec![true; 2];
    inputs.ev_mode = MilpLoadMode::MustRun;

    inputs.p_ev_max_kw = 12.0;
    inputs.p_ev_min_kw = 0.0;
    set_ev_firm(&mut inputs, 12.0, 1);
    inputs.penalty_rules = vec![penalty_rule(10.0, 3600, 5.0)];

    let result = solve_phase1(
        &inputs,
        &make_phase1_weights(),
        &contexts_from_inputs(&inputs),
        60.0,
    );
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
    let mut inputs = make_solver_inputs(1, 0.0);
    inputs.a_ev = vec![true; 1];
    inputs.ev_mode = MilpLoadMode::MustRun;

    inputs.p_ev_max_kw = 12.0;
    inputs.p_ev_min_kw = 0.0;
    set_ev_firm(&mut inputs, 12.0, 0);
    inputs.penalty_rules = vec![penalty_rule(10.0, 3600, 5.0)];

    let result = solve_phase1(
        &inputs,
        &make_phase1_weights(),
        &contexts_from_inputs(&inputs),
        60.0,
    );
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
    let mut inputs = make_solver_inputs(1, 0.0);
    inputs.a_ev = vec![true; 1];
    inputs.ev_mode = MilpLoadMode::MustRun;

    inputs.p_ev_max_kw = 12.0;
    set_ev_firm(&mut inputs, 12.0, 0);
    inputs.penalty_rules = vec![penalty_rule(10.0, 3600, 5.0)];

    let weights = make_phase1_weights();
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
        None,
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
