use good_lp::solvers::highs::highs;
use good_lp::{constraint, Solution, SolverModel, WithInitialSolution, WithMipGap, WithTimeLimit};

use crate::controller::milp_planner::AssetMilpContext;

use super::model_skeleton::{with_constraint, Declaration, ModelSkeleton};
use super::solver_phase1::solve_phase1;
use super::types::*;

/// Phase 2: minimise operational friction subject to phase1_cost(p2_vars) ≤ c_star + epsilon.
/// All variables are declared fresh, each asset with its friction auxiliaries. Phase 1's
/// solution is the warm start: it satisfies every phase 2 constraint, so HiGHS immediately has a
/// feasible integer point, avoiding the NoSolutionFound timeout on Node1 ARM.
#[allow(clippy::too_many_arguments)]
pub(crate) fn solve_phase2(
    inputs: &MilpInputs,
    p1w: &Phase1Weights,
    c_star: f64,
    epsilon: f64,
    phase1_sol: &SolveOutput,
    asset_contexts: &[Box<dyn AssetMilpContext>],
    timeout_s: f64,
) -> Result<(SolveOutput, f64), Box<dyn std::error::Error>> {
    probe!(begin, "solve_phase2");
    let (vars, skeleton) = ModelSkeleton::declare(inputs, p1w, asset_contexts, Declaration::Phase2);

    // Phase 1 cost cap, over this solve's own variables: the very expression phase 1 minimised.
    // Built once, in the skeleton, so `c_star` and the cap cannot disagree by a term (which would
    // make phase 2 infeasible outright). It is an economic cost, not friction, so it belongs in the
    // cap and not in `friction_obj` below.
    let phase1_cap_expr = skeleton.cost_expr(inputs, p1w);
    let friction_obj = skeleton.friction_expr(inputs);
    let warm_start = skeleton.warm_start(phase1_sol);

    probe!(vars, &vars);
    probe!(expr, "friction", &friction_obj);
    probe!(expr, "phase1_cap", &phase1_cap_expr);
    probe!(warm_start, &warm_start);
    let mut model = vars.minimise(&friction_obj).using(highs);
    model = model.with_initial_solution(warm_start);
    model = with_constraint(model, constraint!(phase1_cap_expr <= c_star + epsilon));
    (model, _) = skeleton.add_constraints(model, inputs);
    model = model.with_time_limit(timeout_s);
    model = model.with_mip_gap(inputs.mip_gap_target as f32)?;
    let solution = model.solve()?;

    let friction_value = solution.eval(&friction_obj);
    let out = skeleton.read_output(&solution, &friction_obj, inputs);
    Ok((out, friction_value))
}

/// Lexicographic two-phase wrapper. Phase 1 always runs.
/// Phase 2 runs when `epsilon > 0`; on Phase 2 failure, Phase 1 solution is returned.
/// Returns `(solution, phase1_cost_eur, friction_eur, marginal_cost_eur_per_kwh)`.
/// The marginal-cost vector (see `docs/architecture/VEN_ARCHITECTURE.md`) comes from a second,
/// binaries-fixed LP solve over the winning solution — see `solver_duals::solve_marginal_costs`.
/// It's a read-only diagnostic: a failure there degrades to the plain import tariff per slot
/// rather than failing the whole planning cycle.
#[allow(clippy::type_complexity)] // 4-tuple documented above: solution, phase1_cost_eur, friction_eur, marginal_cost_eur_per_kwh
pub(crate) fn solve_milp_two_phase(
    inputs: &MilpInputs,
    p1w: &Phase1Weights,
    epsilon: f64,
    asset_contexts: &[Box<dyn AssetMilpContext>],
    timeout_s: f64,
    // Phase 2's own budget (R-97): it is inert on heater sites and sub-second
    // elsewhere, so it does not need phase 1's.
    phase2_timeout_s: f64,
) -> Result<
    (
        SolveOutput,
        f64,
        f64,
        Vec<f64>,
        crate::entities::plan::PlanPhaseReport,
    ),
    Box<dyn std::error::Error>,
> {
    // R-97: time and report the phases separately. `Plan.solve_status` carries only
    // the winning solution's status, which collapses two solves that behave nothing
    // alike — since phase 2 got its own (short) budget it reports TimeLimit on
    // essentially every heater cycle by design, making the field useless for
    // telling "phase 1 could not solve this site" from "phase 2 stopped as
    // intended". Until that is on `Plan` itself, this line is the only way to see
    // which half spent the time.
    let t_p1 = std::time::Instant::now();
    let phase1_sol = solve_phase1(inputs, p1w, asset_contexts, timeout_s)?;
    let phase1_ms = t_p1.elapsed().as_millis() as u64;
    let phase1_status_raw = phase1_sol.status;
    let phase1_status = format!("{:?}", phase1_sol.status);
    let c_star = phase1_sol.objective_eur;
    let t_p2 = std::time::Instant::now();
    let mut phase2_status = None;
    let (winning_sol, friction_eur) = if epsilon == 0.0 {
        (phase1_sol, 0.0)
    } else {
        tracing::debug!(c_star, epsilon, "Phase 2 starting");
        match solve_phase2(
            inputs,
            p1w,
            c_star,
            epsilon,
            &phase1_sol,
            asset_contexts,
            phase2_timeout_s,
        ) {
            Ok((sol, friction_eur)) => {
                phase2_status = Some(super::types::map_solve_status(sol.status));
                (sol, friction_eur)
            }
            Err(e) => {
                tracing::warn!(
                    c_star,
                    epsilon,
                    "Phase 2 failed (warm-start provided), using Phase 1: {e}"
                );
                (phase1_sol, 0.0)
            }
        }
    };

    let phase2_ms = t_p2.elapsed().as_millis() as u64;
    // R-97: the same split the log line below reports, carried onto the Plan so it
    // is visible without shell access to a container.
    let phase_report = crate::entities::plan::PlanPhaseReport {
        phase1_ms,
        phase1_status: super::types::map_solve_status(phase1_status_raw),
        phase2_ms,
        phase2_status,
    };
    tracing::info!(
        phase1_ms,
        phase1_status = %phase1_status,
        phase2_ms,
        phase2_status = %format!("{:?}", winning_sol.status),
        phase2_budget_s = phase2_timeout_s,
        phase1_budget_s = timeout_s,
        friction_eur,
        epsilon,
        "planner: phase timings"
    );

    let marginal_cost_eur_per_kwh = match super::solver_duals::solve_marginal_costs(
        inputs,
        p1w,
        asset_contexts,
        &winning_sol,
        timeout_s,
    ) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "marginal-cost dual LP failed, falling back to tariff");
            inputs.c_imp_eur_kwh.clone()
        }
    };

    Ok((
        winning_sol,
        c_star,
        friction_eur,
        marginal_cost_eur_per_kwh,
        phase_report,
    ))
}
