use good_lp::solvers::highs::highs;
use good_lp::{
    constraint, variable, variables, Expression, Solution, SolverModel, Variable,
    WithInitialSolution, WithMipGap, WithTimeLimit,
};

use crate::controller::milp_interactions::{
    build_interactions, GlobalMilpInputs, GridMilpVars, MilpVarPool, ShiftableLoadMilpVars,
};
use crate::controller::milp_planner::{AssetKind, AssetMilpContext};

use super::solver_phase1::{add_model_constraints, read_solve_output, solve_phase1, M_LOW_EUR_PER_KWH};
use super::types::*;

/// Phase 2: minimise operational friction subject to phase1_cost(p2_vars) ≤ c_star + epsilon.
/// All variables are declared fresh. Battery/EV get startup/ramp aux vars.
/// Warm-start vector: Phase 1 solution values provided as initial MIP incumbent for Phase 2.
/// This ensures HiGHS immediately has a feasible integer point (the Phase 1 solution satisfies
/// all Phase 2 constraints), avoiding the NoSolutionFound timeout on Pi4 ARM.
pub(crate) fn build_phase2_warm_start(
    inputs: &MilpInputs,
    p1: &SolveOutput,
    p_imp: &[Variable],
    p_exp: &[Variable],
    u_grid: &[Variable],
    s_imp_viol: &[Variable],
    s_exp_viol: &[Variable],
    pool: &MilpVarPool,
    n: usize,
) -> Vec<(Variable, f64)> {
    let mut iv: Vec<(Variable, f64)> = Vec::with_capacity(n * 12);
    for t in 0..n {
        iv.push((p_imp[t], p1.p_imp_kw[t].max(0.0)));
        iv.push((p_exp[t], p1.p_exp_kw[t].max(0.0)));
        iv.push((u_grid[t], if p1.p_imp_kw[t] > 1e-6 { 1.0 } else { 0.0 }));
        iv.push((s_imp_viol[t], p1.s_imp_viol_kw[t].max(0.0)));
        iv.push((s_exp_viol[t], p1.s_exp_viol_kw[t].max(0.0)));
    }
    if let Some(v) = &pool.heater {
        let iz_mid = inputs.heat_initial_z_mid;
        let iz_full = inputs.heat_initial_z_full;
        for t in 0..n {
            let zm = p1.z_heat_mid[t];
            let zf = p1.z_heat_full[t];
            iv.push((v.z_heat_mid[t], zm));
            iv.push((v.z_heat_full[t], zf));
            let e = p1.e_heat_tank_kwh.get(t).copied().unwrap_or(0.0);
            iv.push((v.e_tank[t], e));
            iv.push((v.s_low[t], (-e).max(0.0)));
            let zm_prev = if t == 0 { iz_mid } else { p1.z_heat_mid[t - 1] };
            let zf_prev = if t == 0 {
                iz_full
            } else {
                p1.z_heat_full[t - 1]
            };
            let sw = (zm - zm_prev).abs().max((zf - zf_prev).abs());
            iv.push((v.sw[t], sw));
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
            let active = if p1.p_bat_ch_kw[t] + p1.p_bat_dis_kw[t] > 1e-6 {
                1.0
            } else {
                0.0
            };
            iv.push((v.u_bat[t], active));
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
        iv.push((v.z_ev_core, p1.z_ev_core));
    }
    iv
}

pub(crate) fn solve_phase2(
    inputs: &MilpInputs,
    p1w: &Phase1Weights,
    p2w: &Phase2Weights,
    c_star: f64,
    epsilon: f64,
    phase1_sol: &SolveOutput,
    asset_contexts: &[Box<dyn AssetMilpContext>],
) -> Result<(SolveOutput, f64), Box<dyn std::error::Error>> {
    let n = inputs.n;
    let dt_h = inputs.dt_h;

    let global = GlobalMilpInputs {
        n,
        dt_h,
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
    let u_grid: Vec<Variable> = (0..n).map(|_| vars.add(variable().binary())).collect();
    let s_imp_viol: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();
    let s_exp_viol: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();
    let grid_vars = GridMilpVars {
        p_imp: p_imp.clone(),
        p_exp: p_exp.clone(),
        u_grid: u_grid.clone(),
        s_imp_viol: s_imp_viol.clone(),
        s_exp_viol: s_exp_viol.clone(),
    };

    let shift_vars: Vec<ShiftableLoadMilpVars> = inputs
        .shiftable_loads
        .iter()
        .map(|sl| {
            let y_shift = sl
                .valid_start_slots
                .iter()
                .map(|_| vars.add(variable().binary()))
                .collect();
            ShiftableLoadMilpVars {
                asset_id: sl.asset_id.clone(),
                power_kw: sl.power_kw,
                duration_slots: sl.duration_slots,
                valid_start_slots: sl.valid_start_slots.clone(),
                y_shift,
            }
        })
        .collect();

    let mut pool = MilpVarPool {
        grid: grid_vars,
        bat: None,
        ev: None,
        heater: None,
        shiftable: shift_vars,
    };

    // Phase 2: per-asset startup/ramp aux vars declared with real cost values.
    for ctx in asset_contexts {
        match ctx.asset_kind() {
            AssetKind::Battery => {
                ctx.declare_vars_into_pool(
                    n, p2w.c_bat_startup_eur, p2w.c_bat_ramp_eur_kw, &mut vars, &mut pool,
                );
            }
            AssetKind::Ev => {
                ctx.declare_vars_into_pool(
                    n, p2w.c_ev_startup_eur, p2w.c_ev_ramp_eur_kw, &mut vars, &mut pool,
                );
            }
            AssetKind::Heater => {
                ctx.declare_vars_into_pool(n, 0.0, 0.0, &mut vars, &mut pool);
            }
        }
    }

    let interactions = build_interactions(p1w.c_bat_ev_coexist_eur_kwh);
    let mut active_interactions: Vec<
        &Box<dyn crate::controller::milp_interactions::AssetInteraction>,
    > = Vec::new();
    let mut iv_list: Vec<crate::controller::milp_interactions::InteractionVars> = Vec::new();
    for interaction in &interactions {
        if interaction.applicable(&pool) {
            let iv = interaction.declare_vars(&pool, &global, &mut vars);
            active_interactions.push(interaction);
            iv_list.push(iv);
        }
    }

    // Phase 1 cost cap expression, rebuilt using Phase 2 variables.
    let mut phase1_cap_expr = Expression::from(0.0);
    for t in 0..n {
        phase1_cap_expr += (p1w.w_energy * dt_h * inputs.c_imp_eur_kwh[t]) * p_imp[t];
        phase1_cap_expr += -(p1w.w_energy * dt_h * inputs.c_exp_eur_kwh[t]) * p_exp[t];
        phase1_cap_expr += (p1w.w_ghg * dt_h * inputs.g_imp_kgco2_kwh[t]) * p_imp[t];
        phase1_cap_expr += (p1w.w_grid * dt_h) * p_imp[t];
        phase1_cap_expr += (p1w.w_grid * dt_h) * p_exp[t];
        phase1_cap_expr += (p1w.w_import * dt_h) * p_imp[t];
        phase1_cap_expr += (p1w.w_viol * inputs.pen_imp_eur_kwh * dt_h) * s_imp_viol[t];
        phase1_cap_expr += (p1w.w_viol * inputs.pen_exp_eur_kwh * dt_h) * s_exp_viol[t];
    }
    // Phase 1 cost cap contributions: battery wear + EV service reward + heater m_low.
    // Matches Phase 1 objective exactly so the cap is meaningful.
    for ctx in asset_contexts {
        match ctx.asset_kind() {
            AssetKind::Battery => {
                // Battery wear only (c_startup=0, c_ramp=0 in cost cap).
                phase1_cap_expr +=
                    ctx.objective(&pool, n, dt_h, p1w.c_bat_wear_eur_kwh, 0.0, 0.0);
            }
            AssetKind::Ev => {
                // EV service reward only (c_startup=0 → Phase 1 mode in EV impl).
                phase1_cap_expr += ctx.objective(&pool, n, dt_h, 0.0, 0.0, 0.0);
            }
            AssetKind::Heater => {
                // m_low term: use Phase 1 convention (c_startup=0 → m_low in heater impl).
                phase1_cap_expr += ctx.objective(&pool, n, dt_h, 0.0, 0.0, 0.0);
            }
        }
    }
    for (interaction, iv) in active_interactions.iter().zip(iv_list.iter()) {
        phase1_cap_expr += interaction.objective(iv, dt_h);
    }

    // Phase 2 friction objective: startup/ramp/switching/tier; no economic terms.
    // c_startup_eur > 0.0 signals Phase 2 mode to asset objective impls.
    let mut friction_obj = Expression::from(0.0);
    for ctx in asset_contexts {
        match ctx.asset_kind() {
            AssetKind::Battery => {
                // wear=0, startup=bat_startup, ramp=bat_ramp.
                friction_obj += ctx.objective(
                    &pool, n, dt_h, 0.0, p2w.c_bat_startup_eur, p2w.c_bat_ramp_eur_kw,
                );
            }
            AssetKind::Ev => {
                // c_startup>0 → Phase 2: startup+ramp active, service reward off.
                friction_obj += ctx.objective(
                    &pool, n, dt_h, 0.0, p2w.c_ev_startup_eur, p2w.c_ev_ramp_eur_kw,
                );
            }
            AssetKind::Heater => {
                // c_startup=1.0 signals Phase 2; c_ramp carries w_tier_penalty_eur.
                // self.lambda_sw_eur applied internally by HeaterMilpContext::objective.
                friction_obj += ctx.objective(
                    &pool, n, dt_h, 0.0, 1.0, p2w.w_tier_penalty_eur,
                );
            }
        }
    }

    let warm_start = build_phase2_warm_start(
        inputs,
        phase1_sol,
        &p_imp,
        &p_exp,
        &u_grid,
        &s_imp_viol,
        &s_exp_viol,
        &pool,
        n,
    );

    let mut model = vars.minimise(&friction_obj).using(highs);
    model = model.with_initial_solution(warm_start);
    model = model.with(constraint!(phase1_cap_expr <= c_star + epsilon));
    model = add_model_constraints(
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
    );
    model = model.with_time_limit(60.0);
    model = model.with_mip_gap(0.02)?;
    let solution = model.solve()?;

    let friction_value = solution.eval(&friction_obj);
    let out = read_solve_output(&solution, &friction_obj, &pool, inputs, n);
    Ok((out, friction_value))
}

/// Lexicographic two-phase wrapper. Phase 1 always runs.
/// Phase 2 runs when `epsilon > 0`; on Phase 2 failure, Phase 1 solution is returned.
/// Returns `(solution, phase1_cost_eur, friction_eur)`.
pub(crate) fn solve_milp_two_phase(
    inputs: &MilpInputs,
    p1w: &Phase1Weights,
    p2w: &Phase2Weights,
    epsilon: f64,
    asset_contexts: &[Box<dyn AssetMilpContext>],
) -> Result<(SolveOutput, f64, f64), Box<dyn std::error::Error>> {
    let phase1_sol = solve_phase1(inputs, p1w, asset_contexts)?;
    let c_star = phase1_sol.objective_eur;
    if epsilon == 0.0 {
        return Ok((phase1_sol, c_star, 0.0));
    }
    tracing::debug!(c_star, epsilon, "Phase 2 starting");
    match solve_phase2(inputs, p1w, p2w, c_star, epsilon, &phase1_sol, asset_contexts) {
        Ok((sol, friction_eur)) => Ok((sol, c_star, friction_eur)),
        Err(e) => {
            tracing::warn!(
                c_star,
                epsilon,
                "Phase 2 failed (warm-start provided), using Phase 1: {e}"
            );
            Ok((phase1_sol, c_star, 0.0))
        }
    }
}
