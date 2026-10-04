//! Second, cheap LP solve per planning cycle (see `docs/architecture/VEN_ARCHITECTURE.md`):
//! fixes every binary decision to the winning MILP solution's value and re-solves as a pure LP to
//! read a real shadow price off each slot's power-balance row.
//!
//! Raw MILP duals aren't meaningful once integers are involved — and, critically, HiGHS never
//! populates row/column duals at all for a model that has *any* integer-flagged column, even one
//! pinned to a single value via an equality constraint (confirmed empirically: adding
//! `constraint!(u_grid[t] == 1.0)` on top of a `variable().binary()` column still returns an
//! all-zero dual vector). So fixing a decision here means declaring it as a genuinely continuous
//! variable with `min == max == winning_value`, not adding a constraint on top of a binary
//! declaration. Every other (truly continuous) decision — power levels, SoC/tank trajectories —
//! stays free within its normal bounds, so the remaining LP still has real degrees of freedom for
//! HiGHS to price.
//!
//! Every asset declares its own variables through
//! `AssetMilpContext::declare_pinned_vars_into_pool` — the same declaration the plan used, with
//! its mode decisions pinned to `SolveOutput::mode_decisions` (R-98). A second, hand-written
//! declaration here used to drift from the real one until the LP was infeasible on every VEN.
//! The *same* `constraints()`/`objective()` trait methods are then called against this pool.
//! `good_lp::Variable` is just an opaque id, so those methods don't care whether the variable
//! behind it was declared integer or continuous.
//!
//! Read-only diagnostic: this never feeds back into `p_imp`/`p_exp`/allocations — see
//! `solve_marginal_costs`'s caller (`solve_milp_two_phase`).

use good_lp::solvers::highs::highs;
use good_lp::solvers::{DualValues, SolutionWithDual};
use good_lp::{variable, variables, Expression, SolverModel, Variable, WithMipGap, WithTimeLimit};

use crate::controller::milp_interactions::{
    build_interactions, pv_use_tiebreak_expr, shiftable_tiebreak_expr, GlobalMilpInputs,
    GridMilpVars, MilpVarPool,
};
use crate::controller::milp_planner::asset_port::pinned_binary;
use crate::controller::milp_planner::{AssetKind, AssetMilpContext};

use super::penalty;
use super::solver_phase1::add_model_constraints;
use super::types::*;

/// Fix every binary decision in a freshly-declared (all-continuous) pool to `winning`'s rounded
/// value, then read the power-balance dual for each slot. Returns one value per slot — both the
/// import- and export-side shadow price for now (§5.2: a harmless simplification under
/// cost-minimizing objectives, where the tariff curve is close to linear through zero; splitting
/// them is deferred to when an objective with a genuine kink at zero net exchange needs it).
pub(crate) fn solve_marginal_costs(
    inputs: &MilpInputs,
    p1w: &Phase1Weights,
    asset_contexts: &[Box<dyn AssetMilpContext>],
    winning: &SolveOutput,
    timeout_s: f64,
) -> Result<Vec<f64>, Box<dyn std::error::Error>> {
    let n = inputs.n;

    let global = GlobalMilpInputs {
        n,
        dt_h: inputs.dt_h.clone(),
        c_imp_eur_kwh: inputs.c_imp_eur_kwh.clone(),
        c_exp_eur_kwh: inputs.c_exp_eur_kwh.clone(),
        g_imp_kgco2_kwh: inputs.g_imp_kgco2_kwh.clone(),
        p_pv_kw: inputs.p_pv_kw.clone(),
        p_base_kw: inputs.p_base_kw.clone(),
        p_imp_max_phys_kw: inputs.p_imp_max_phys_kw.clone(),
        p_exp_max_phys_kw: inputs.p_exp_max_phys_kw.clone(),
        p_imp_max_cont_kw: inputs.p_imp_max_cont_kw.clone(),
        p_exp_max_cont_kw: inputs.p_exp_max_cont_kw.clone(),
        pen_imp_eur_kwh: inputs.pen_imp_eur_kwh,
        pen_exp_eur_kwh: inputs.pen_exp_eur_kwh,
    };

    let mut vars = variables!();

    let p_imp: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();
    let p_exp: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();
    // u_grid is a mode decision like the asset binaries below — fixed continuous, not
    // `.binary()`, for the same reason (see module doc).
    let u_grid: Vec<Variable> = (0..n)
        .map(|t| {
            let v = pinned_binary(Some(if winning.p_imp_kw[t] > 1e-6 { 1.0 } else { 0.0 }));
            vars.add(variable().min(v).max(v))
        })
        .collect();
    let s_imp_viol: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();
    let s_exp_viol: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();
    let p_pv_used: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();
    let grid_vars = GridMilpVars {
        p_imp: p_imp.clone(),
        p_exp: p_exp.clone(),
        u_grid: u_grid.clone(),
        s_imp_viol: s_imp_viol.clone(),
        s_exp_viol: s_exp_viol.clone(),
        p_pv_used: p_pv_used.clone(),
    };

    let mut pool = MilpVarPool {
        grid: grid_vars,
        bat: None,
        ev: None,
        heater: None,
        shiftable: Vec::new(),
    };

    // WP6.3 (BL-09): declared fresh (not fixed to `winning`, like s_imp_viol above) —
    // this is a free continuous slack, not a mode decision, so it needs no rounding fix.
    let penalty_vars =
        penalty::declare_penalty_vars(&inputs.penalty_rules, &inputs.cum_s, &mut vars);

    // R-98: every asset declares its own variables, through the same function the plan
    // used, with the winning mode decisions pinned — so the priced model is the planned one.
    for ctx in asset_contexts {
        ctx.declare_pinned_vars_into_pool(n, &winning.mode_decisions, &mut vars, &mut pool);
    }

    let interactions =
        build_interactions(p1w.c_bat_ev_coexist_eur_kwh, p1w.c_ctrl_imp_malus_eur_kwh);
    let mut active_interactions: Vec<&dyn crate::controller::milp_interactions::AssetInteraction> =
        Vec::new();
    let mut iv_list: Vec<crate::controller::milp_interactions::InteractionVars> = Vec::new();
    for interaction in &interactions {
        if interaction.applicable(&pool) {
            let iv = interaction.declare_vars(&pool, &global, &mut vars);
            active_interactions.push(interaction.as_ref());
            iv_list.push(iv);
        }
    }

    // Mirrors solve_phase1's objective exactly — the dual must reflect the real
    // objective's sensitivity, not an arbitrary one.
    let mut objective = Expression::from(0.0);
    for t in 0..n {
        objective += (p1w.w_energy * inputs.dt_h[t] * inputs.c_imp_eur_kwh[t]) * p_imp[t];
        objective += -(p1w.w_energy * inputs.dt_h[t] * inputs.c_exp_eur_kwh[t]) * p_exp[t];
        objective += (p1w.w_ghg * inputs.dt_h[t] * inputs.g_imp_kgco2_kwh[t]) * p_imp[t];
        objective += (p1w.w_grid * inputs.dt_h[t]) * p_imp[t];
        objective += (p1w.w_grid * inputs.dt_h[t]) * p_exp[t];
        objective += (p1w.w_import * inputs.dt_h[t]) * p_imp[t];
        objective += (p1w.w_viol * inputs.pen_imp_eur_kwh * inputs.dt_h[t]) * s_imp_viol[t];
        objective += (p1w.w_viol * inputs.pen_exp_eur_kwh * inputs.dt_h[t]) * s_exp_viol[t];
    }
    // WP6.3 (BL-09): mirrors solve_phase1's objective exactly, same rationale as
    // this module's other terms (module doc: the dual must reflect the real
    // objective's sensitivity).
    objective += penalty::penalty_objective(&penalty_vars);
    for ctx in asset_contexts {
        match ctx.asset_kind() {
            AssetKind::Battery => {
                objective +=
                    ctx.objective(&pool, n, &inputs.dt_h, p1w.c_bat_wear_eur_kwh, 0.0, 0.0);
            }
            AssetKind::Ev => {
                objective += ctx.objective(&pool, n, &inputs.dt_h, 0.0, 0.0, 0.0);
            }
            AssetKind::Heater => {
                objective += ctx.objective(&pool, n, &inputs.dt_h, 0.0, 0.0, 0.0);
            }
            AssetKind::ShiftableLoad => {
                objective += ctx.objective(&pool, n, &inputs.dt_h, 0.0, 0.0, 0.0);
            }
        }
    }
    for (interaction, iv) in active_interactions.iter().zip(iv_list.iter()) {
        objective += interaction.objective(iv, &inputs.dt_h);
    }
    objective += shiftable_tiebreak_expr(&pool.shiftable);
    objective += pv_use_tiebreak_expr(&pool.grid, &inputs.dt_h);

    let model = vars.minimise(&objective).using(highs);

    let (model, power_balance_refs) = add_model_constraints(
        model,
        inputs,
        &pool,
        &p_imp,
        &p_exp,
        &u_grid,
        &s_imp_viol,
        &s_exp_viol,
        &active_interactions,
        &iv_list,
        &global,
        asset_contexts,
        n,
        &penalty_vars,
    );
    let model = model.with_time_limit(timeout_s);
    let model = model.with_mip_gap(inputs.mip_gap_target as f32)?;
    let mut solution = model.solve()?;
    let dual = solution.compute_dual();

    Ok(power_balance_refs
        .into_iter()
        .map(|r| dual.dual(r))
        .collect())
}
