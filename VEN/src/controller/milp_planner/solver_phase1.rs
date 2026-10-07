use good_lp::solvers::highs::highs;
use good_lp::{SolverModel, WithMipGap, WithTimeLimit};

use super::model_skeleton::{Declaration, ModelSkeleton};
use super::types::*;
use crate::controller::milp_planner::AssetMilpContext;

/// Phase 1: minimise economic cost only. Battery and EV declared without startup/ramp aux vars.
/// Heater objective uses m_low penalty only (lambda_sw=0 via c_startup_eur=0.0 convention).
pub(crate) fn solve_phase1(
    inputs: &MilpInputs,
    p1w: &Phase1Weights,
    asset_contexts: &[Box<dyn AssetMilpContext>],
    timeout_s: f64,
) -> Result<SolveOutput, Box<dyn std::error::Error>> {
    probe!(begin, "solve_phase1");
    let (vars, skeleton) = ModelSkeleton::declare(inputs, p1w, asset_contexts, Declaration::Phase1);
    let objective = skeleton.cost_expr(inputs, p1w, asset_contexts);

    probe!(vars, &vars);
    probe!(expr, "objective", &objective);
    let model = vars.minimise(&objective).using(highs);
    let (mut model, _) = skeleton.add_constraints(model, inputs, asset_contexts);
    model = model.with_time_limit(timeout_s);
    model = model.with_mip_gap(inputs.mip_gap_target as f32)?;
    let solution = model.solve()?;

    Ok(skeleton.read_output(&solution, &objective, inputs))
}
