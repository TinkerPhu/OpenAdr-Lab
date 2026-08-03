//! WP6.3 (BL-09) — peak-demand penalty threshold check.
//!
//! Per-solve, per-window soft-penalty MILP term: for each configured
//! [`PenaltyRuleParams`], one shared slack variable per fixed, horizon-aligned
//! window bounds every slot's grid import in that window at `threshold_kw`,
//! penalized in the objective at `penalty_eur_per_kw`. Mirrors the existing
//! `s_imp_viol` soft-constraint idiom in `solver_phase1.rs`.
//!
//! Deliberately *not* the stateful, persisted billing-period tracker sketched
//! in `entities::design_vocabulary::PenaltyRule` (rolling averages,
//! `breached_this_period` surviving restarts) — each solve re-evaluates its
//! own horizon fresh. See `openspec/changes/penalty-threshold-check/design.md`
//! (Decisions D1-D4) for the rejected alternatives.

use good_lp::{constraint, variable, Constraint, Expression, ProblemVariables, Solution, Variable};

use crate::entities::planner_params::PenaltyRuleParams;

/// Number of fixed windows of `window_s` needed to cover a horizon whose
/// slot-boundary offsets are `cum_s` (len n+1, seconds from horizon start).
/// Uses `cum_s`'s exact integer arithmetic rather than re-deriving from
/// `dt_h`, per the precision rule documented on `MilpInputs::cum_s`.
pub(crate) fn num_windows(cum_s: &[i64], window_s: u64) -> usize {
    let total_s = cum_s.last().copied().unwrap_or(0);
    if total_s <= 0 || window_s == 0 {
        return 0;
    }
    // Manual ceil-div (avoids relying on i64::div_ceil's MSRV).
    (((total_s - 1) / window_s as i64) + 1) as usize
}

/// Which fixed window slot `t` falls into, given its start offset `cum_s[t]`.
pub(crate) fn window_index(cum_s: &[i64], t: usize, window_s: u64) -> usize {
    (cum_s[t] / window_s as i64) as usize
}

/// One rule's declared MILP slack variables — one non-negative slack per
/// fixed window.
#[derive(Clone)]
pub(crate) struct PenaltyRuleVars {
    pub(crate) rule: PenaltyRuleParams,
    pub(crate) s_penalty: Vec<Variable>,
}

/// Declare one slack variable per window, for each active rule. Empty
/// `rules` (the default) declares nothing — the feature is a no-op.
pub(crate) fn declare_penalty_vars(
    rules: &[PenaltyRuleParams],
    cum_s: &[i64],
    vars: &mut ProblemVariables,
) -> Vec<PenaltyRuleVars> {
    rules
        .iter()
        .map(|rule| {
            let nw = num_windows(cum_s, rule.measurement_window_s);
            let s_penalty = (0..nw).map(|_| vars.add(variable().min(0.0))).collect();
            PenaltyRuleVars {
                rule: rule.clone(),
                s_penalty,
            }
        })
        .collect()
}

/// `p_imp[t] <= threshold_kw + s_penalty[window_of(t)]` for every slot and
/// every active rule.
pub(crate) fn penalty_constraints(
    p_imp: &[Variable],
    cum_s: &[i64],
    rule_vars: &[PenaltyRuleVars],
) -> Vec<Constraint> {
    let mut out = Vec::new();
    for rv in rule_vars {
        for (t, &p) in p_imp.iter().enumerate() {
            let w = window_index(cum_s, t, rv.rule.measurement_window_s);
            out.push(constraint!(p <= rv.rule.threshold_kw + rv.s_penalty[w]));
        }
    }
    out
}

/// Objective contribution: `penalty_eur_per_kw` per kW of window slack, once
/// per window — a demand-charge-style peak cost, not an energy cost, so
/// (unlike `s_imp_viol`'s per-slot violation penalty) there is no `dt_h` factor.
pub(crate) fn penalty_objective(rule_vars: &[PenaltyRuleVars]) -> Expression {
    let mut obj = Expression::from(0.0);
    for rv in rule_vars {
        for &s in &rv.s_penalty {
            obj += rv.rule.penalty_eur_per_kw * s;
        }
    }
    obj
}

/// Read solved slack values back out — outer `Vec` per rule (in `rule_vars`
/// order), inner per that rule's window.
pub(crate) fn read_penalty_solution<S: Solution>(
    solution: &S,
    rule_vars: &[PenaltyRuleVars],
) -> Vec<Vec<f64>> {
    rule_vars
        .iter()
        .map(|rv| rv.s_penalty.iter().map(|&s| solution.value(s)).collect())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn num_windows_buckets_correctly() {
        // 24h horizon, 5-min steps (288 slots), measurement_window_s=1800 (30 min)
        // -> 48 windows of 6 slots each.
        let cum_s: Vec<i64> = (0..=288).map(|i| i * 300).collect();
        assert_eq!(num_windows(&cum_s, 1800), 48);
    }

    #[test]
    fn num_windows_is_zero_when_horizon_or_window_is_zero() {
        assert_eq!(num_windows(&[], 300), 0);
        assert_eq!(num_windows(&[0], 300), 0);
        assert_eq!(num_windows(&[0, 300], 0), 0);
    }

    #[test]
    fn window_index_maps_slots_to_expected_bucket() {
        let cum_s: Vec<i64> = (0..=12).map(|i| i * 300).collect(); // 5-min slots
                                                                   // window_s = 1800 (6 slots per window)
        assert_eq!(window_index(&cum_s, 0, 1800), 0);
        assert_eq!(window_index(&cum_s, 5, 1800), 0);
        assert_eq!(window_index(&cum_s, 6, 1800), 1);
        assert_eq!(window_index(&cum_s, 11, 1800), 1);
    }

    #[test]
    fn window_index_one_window_per_slot_when_window_equals_step() {
        let cum_s: Vec<i64> = (0..=4).map(|i| i * 300).collect();
        for t in 0..4 {
            assert_eq!(window_index(&cum_s, t, 300), t);
        }
    }
}
