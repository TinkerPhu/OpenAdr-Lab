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
use good_lp::{SolverModel, WithMipGap, WithTimeLimit};

use super::model_skeleton::{Declaration, ModelSkeleton};
use super::types::*;
use crate::controller::milp_planner::AssetMilpContext;

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
    probe!(begin, "solve_marginal_costs");
    let (vars, skeleton) =
        ModelSkeleton::declare(inputs, p1w, asset_contexts, Declaration::Pinned(winning));
    // The same economic cost phase 1 minimised: the dual must reflect the real objective's
    // sensitivity, not an arbitrary one.
    let objective = skeleton.cost_expr(inputs, p1w, asset_contexts);

    probe!(vars, &vars);
    probe!(expr, "objective", &objective);
    let model = vars.minimise(&objective).using(highs);
    let (model, power_balance_refs) = skeleton.add_constraints(model, inputs, asset_contexts);
    let model = model.with_time_limit(timeout_s);
    let model = model.with_mip_gap(inputs.mip_gap_target as f32)?;
    let mut solution = model.solve()?;
    let dual = solution.compute_dual();

    Ok(power_balance_refs
        .into_iter()
        .map(|r| dual.dual(r))
        .collect())
}
