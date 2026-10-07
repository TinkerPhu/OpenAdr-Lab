use good_lp::solvers::highs::highs;
use good_lp::{
    constraint, Expression, Solution, SolverModel, Variable, WithInitialSolution, WithMipGap,
    WithTimeLimit,
};

use crate::controller::milp_interactions::{
    pv_use_tiebreak_expr, shiftable_tiebreak_expr, MilpVarPool,
};
use crate::controller::milp_planner::{AssetKind, AssetMilpContext};

use super::model_skeleton::{with_constraint, Declaration, ModelSkeleton};
use super::solver_phase1::solve_phase1;
use super::types::*;

/// Phase 2: minimise operational friction subject to phase1_cost(p2_vars) ≤ c_star + epsilon.
/// All variables are declared fresh. Battery/EV get startup/ramp aux vars.
/// Warm-start vector: Phase 1 solution values provided as initial MIP incumbent for Phase 2.
/// This ensures HiGHS immediately has a feasible integer point (the Phase 1 solution satisfies
/// all Phase 2 constraints), avoiding the NoSolutionFound timeout on Node1 ARM.
pub(crate) fn build_phase2_warm_start(
    inputs: &MilpInputs,
    p1: &SolveOutput,
    pool: &MilpVarPool,
    n: usize,
) -> Vec<(Variable, f64)> {
    let grid = &pool.grid;
    let mut iv: Vec<(Variable, f64)> = Vec::with_capacity(n * 12);
    for t in 0..n {
        iv.push((grid.p_imp[t], p1.p_imp_kw[t].max(0.0)));
        iv.push((grid.p_exp[t], p1.p_exp_kw[t].max(0.0)));
        iv.push((
            grid.u_grid[t],
            if p1.p_imp_kw[t] > 1e-6 { 1.0 } else { 0.0 },
        ));
        iv.push((grid.s_imp_viol[t], p1.s_imp_viol_kw[t].max(0.0)));
        iv.push((grid.s_exp_viol[t], p1.s_exp_viol_kw[t].max(0.0)));
        iv.push((grid.p_pv_used[t], p1.p_pv_used_kw[t].max(0.0)));
    }
    if let Some(v) = &pool.heater {
        let iy = inputs.heat_initial_y;
        for t in 0..n {
            let y = p1.y_heat[t];
            iv.push((v.y_heat[t], y));
            let e = p1.e_heat_tank_kwh.get(t).copied().unwrap_or(0.0);
            iv.push((v.e_tank[t], e));
            iv.push((v.s_low[t], (-e).max(0.0)));
            let y_prev = if t == 0 { iy } else { p1.y_heat[t - 1] };
            // Matches C5: sw is the relay-operation count, so a two-stage jump
            // seeds 2 rather than the 1 the old max-of-two-binaries gave.
            iv.push((v.sw[t], (y - y_prev).abs()));
        }
        iv.push((v.z_heat_ready, p1.z_heat_ready));
    }
    if let Some(v) = &pool.bat {
        for t in 0..=n {
            if let (Some(&e_var), Some(&e_val)) = (v.e_bat.get(t), p1.e_bat_kwh.get(t)) {
                iv.push((e_var, e_val));
            }
        }
        for t in 0..n {
            iv.push((v.p_ch[t], p1.p_bat_ch_kw[t].max(0.0)));
            iv.push((v.p_dis[t], p1.p_bat_dis_kw[t].max(0.0)));
            // R-97: `u_bat` is the battery's *direction selector*, not an activity
            // flag — `p_ch <= ch_max * u_bat` and `p_dis <= dis_max * (1 - u_bat)`
            // (`battery_milp.rs`). Seeding it from an activity test asserted
            // "charging" in every discharging slot, which makes
            // `p_dis <= dis_max * (1 - 1) = 0` contradict the warm start's own
            // `p_dis > 0`. Phase 2 was therefore handed an infeasible start and had
            // to solve from scratch: survivable on an easy instance, and on
            // heater+battery it returned `NoSolutionFound` on essentially every
            // cycle (ven-5/ven-14/ven-17, 29-33 failures per 3 h in production).
            // Idle slots may take either value; 0 is chosen so an idle battery is
            // not nudged toward charging.
            let charging = if p1.p_bat_ch_kw[t] > 1e-6 { 1.0 } else { 0.0 };
            iv.push((v.u_bat[t], charging));
            // `z_active` genuinely is an activity flag
            // (`p_ch + p_dis <= big_m * z_active`), so it keeps the activity value.
            let active = if p1.p_bat_ch_kw[t] + p1.p_bat_dis_kw[t] > 1e-6 {
                1.0
            } else {
                0.0
            };
            if let Some(&za) = v.z_active.get(t) {
                iv.push((za, active));
            }
        }
        for i in 0..v.delta_active.len() {
            let t = i + 1;
            let z_prev = if p1.p_bat_ch_kw[i] + p1.p_bat_dis_kw[i] > 1e-6 {
                1.0_f64
            } else {
                0.0
            };
            let z_curr = if p1.p_bat_ch_kw[t] + p1.p_bat_dis_kw[t] > 1e-6 {
                1.0_f64
            } else {
                0.0
            };
            iv.push((v.delta_active[i], (z_curr - z_prev).max(0.0)));
        }
        for i in 0..v.delta_ramp.len() {
            let t = i + 1;
            let net_prev = p1.p_bat_ch_kw[i] - p1.p_bat_dis_kw[i];
            let net_curr = p1.p_bat_ch_kw[t] - p1.p_bat_dis_kw[t];
            iv.push((v.delta_ramp[i], (net_curr - net_prev).abs()));
        }
    }
    if let Some(v) = &pool.ev {
        for t in 0..n {
            iv.push((v.p_ev[t], p1.p_ev_kw[t].max(0.0)));
            iv.push((v.z_ev_on[t], p1.z_ev_on[t]));
        }
        for i in 0..v.delta_ev.len() {
            let t = i + 1;
            let delta = (p1.z_ev_on[t] - p1.z_ev_on[i]).max(0.0);
            iv.push((v.delta_ev[i], delta));
        }
        for i in 0..v.delta_ev_ramp.len() {
            let t = i + 1;
            iv.push((v.delta_ev_ramp[i], (p1.p_ev_kw[t] - p1.p_ev_kw[i]).abs()));
        }
        iv.push((v.e_ev_extra, p1.e_ev_extra.max(0.0)));
    }
    iv
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn solve_phase2(
    inputs: &MilpInputs,
    p1w: &Phase1Weights,
    p2w: &Phase2Weights,
    c_star: f64,
    epsilon: f64,
    phase1_sol: &SolveOutput,
    asset_contexts: &[Box<dyn AssetMilpContext>],
    timeout_s: f64,
) -> Result<(SolveOutput, f64), Box<dyn std::error::Error>> {
    probe!(begin, "solve_phase2");
    let n = inputs.n;
    let (vars, skeleton) =
        ModelSkeleton::declare(inputs, p1w, asset_contexts, Declaration::Phase2(p2w));

    // Phase 1 cost cap, over this solve's own variables: the very expression phase 1 minimised.
    // Built once, in the skeleton, so `c_star` and the cap cannot disagree by a term (which would
    // make phase 2 infeasible outright). It is an economic cost, not friction, so it belongs in the
    // cap and not in `friction_obj` below.
    let phase1_cap_expr = skeleton.cost_expr(inputs, p1w, asset_contexts);

    // Phase 2 friction objective: startup/ramp/switching/tier; no economic terms.
    // c_startup_eur > 0.0 signals Phase 2 mode to asset objective impls.
    let mut friction_obj = Expression::from(0.0);
    for ctx in asset_contexts {
        match ctx.asset_kind() {
            AssetKind::Battery => {
                // wear=0, startup=bat_startup, ramp=bat_ramp.
                friction_obj += ctx.objective(
                    &skeleton.pool,
                    n,
                    &inputs.dt_h,
                    0.0,
                    p2w.c_bat_startup_eur,
                    p2w.c_bat_ramp_eur_kw,
                );
            }
            AssetKind::Ev => {
                // c_startup>0 → Phase 2: startup+ramp active, service reward off.
                friction_obj += ctx.objective(
                    &skeleton.pool,
                    n,
                    &inputs.dt_h,
                    0.0,
                    p2w.c_ev_startup_eur,
                    p2w.c_ev_ramp_eur_kw,
                );
            }
            AssetKind::Heater => {
                // c_startup=1.0 signals Phase 2; c_ramp carries w_tier_penalty_eur.
                // self.lambda_sw_eur applied internally by HeaterMilpContext::objective.
                friction_obj += ctx.objective(
                    &skeleton.pool,
                    n,
                    &inputs.dt_h,
                    0.0,
                    1.0,
                    p2w.w_tier_penalty_eur,
                );
            }
            AssetKind::ShiftableLoad => {
                friction_obj += ctx.objective(&skeleton.pool, n, &inputs.dt_h, 0.0, 0.0, 0.0);
            }
        }
    }
    // Earliest-start tie-break must also bias Phase 2: the epsilon cost budget
    // would otherwise let friction smoothing move a shiftable start to a later
    // cost-equal slot, undoing the Phase 1 choice (same lesson as ASAP_FREE).
    friction_obj += shiftable_tiebreak_expr(&skeleton.pool.shiftable);
    // Phase 2's objective is friction-only and otherwise has no opinion on
    // p_pv_used at all — without this, the epsilon cost budget could let
    // friction smoothing curtail PV arbitrarily. Same rationale as the
    // shiftable-load tie-break above.
    friction_obj += pv_use_tiebreak_expr(&skeleton.pool.grid, &inputs.dt_h);

    let warm_start = build_phase2_warm_start(inputs, phase1_sol, &skeleton.pool, n);

    probe!(vars, &vars);
    probe!(expr, "friction", &friction_obj);
    probe!(expr, "phase1_cap", &phase1_cap_expr);
    probe!(warm_start, &warm_start);
    let mut model = vars.minimise(&friction_obj).using(highs);
    model = model.with_initial_solution(warm_start);
    model = with_constraint(model, constraint!(phase1_cap_expr <= c_star + epsilon));
    (model, _) = skeleton.add_constraints(model, inputs, asset_contexts);
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
    p2w: &Phase2Weights,
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
            p2w,
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

/// R-97 test seam: build phase 2's variable pool and warm start exactly as
/// `solve_phase2` does, and hand back the battery's `u_bat` handles so a test can
/// check what value the warm start assigns them. Exists because an infeasible warm
/// start is invisible from the outside — it shows up only as `NoSolutionFound` on
/// instances hard enough that phase 2 cannot recover by solving from scratch.
#[cfg(test)]
pub(crate) fn warm_start_for_test(
    inputs: &MilpInputs,
    p1: &SolveOutput,
    asset_contexts: &[Box<dyn AssetMilpContext>],
) -> (Vec<(Variable, f64)>, Vec<Variable>) {
    use crate::controller::milp_interactions::GridMilpVars;
    use good_lp::{variable, variables};
    let n = inputs.n;
    let mut vars = variables!();
    let p_imp: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();
    let p_exp: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();
    let u_grid: Vec<Variable> = (0..n).map(|_| vars.add(variable().binary())).collect();
    let s_imp_viol: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();
    let s_exp_viol: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();
    let p_pv_used: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();
    let mut pool = MilpVarPool {
        grid: GridMilpVars {
            p_imp: p_imp.clone(),
            p_exp: p_exp.clone(),
            u_grid: u_grid.clone(),
            s_imp_viol: s_imp_viol.clone(),
            s_exp_viol: s_exp_viol.clone(),
            p_pv_used,
        },
        bat: None,
        ev: None,
        heater: None,
        shiftable: Vec::new(),
    };
    // Same non-zero startup/ramp costs phase 2 uses, so the same aux vars exist.
    for ctx in asset_contexts {
        ctx.declare_vars_into_pool(n, 0.01, 0.005, &mut vars, &mut pool);
    }
    let u_bat = pool
        .bat
        .as_ref()
        .map(|b| b.u_bat.clone())
        .unwrap_or_default();
    let iv = build_phase2_warm_start(inputs, p1, &pool, n);
    (iv, u_bat)
}
