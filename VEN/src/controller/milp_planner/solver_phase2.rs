use good_lp::solvers::highs::highs;
use good_lp::{
    constraint, variable, variables, Expression, Solution, SolverModel, Variable,
    WithInitialSolution, WithMipGap, WithTimeLimit,
};

use super::asset_port::{BatteryMilpContext, EvMilpContext, EvMilpMode, HeaterMilpContext, HeaterMilpMode};
use crate::controller::milp_interactions::{
    build_interactions, GlobalMilpInputs, GridMilpVars, MilpVarPool, ShiftableLoadMilpVars,
};

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
) -> Result<(SolveOutput, f64), Box<dyn std::error::Error>> {
    let n = inputs.n;
    let dt_h = inputs.dt_h;

    let bat_ctx: Option<BatteryMilpContext> =
        inputs.e_bat_nom_kwh.map(|e_nom| BatteryMilpContext {
            e_nom_kwh: e_nom,
            e_init_kwh: inputs.e_bat_init_kwh.unwrap_or(0.0),
            e_min_kwh: inputs.e_bat_min_kwh.unwrap_or(0.0),
            e_max_kwh: inputs.e_bat_max_kwh.unwrap_or(e_nom),
            p_ch_max_kw: inputs.p_bat_ch_max_kw.unwrap_or(0.0),
            p_dis_max_kw: inputs.p_bat_dis_max_kw.unwrap_or(0.0),
            eff_ch: inputs.eff_bat_ch.unwrap_or(1.0),
            eff_dis: inputs.eff_bat_dis.unwrap_or(1.0),
        });

    let ev_ctx: Option<EvMilpContext> = if inputs.p_ev_max_kw > 0.0 {
        let ev_mode = match inputs.ev_mode {
            MilpLoadMode::MustRun => EvMilpMode::MustRun,
            MilpLoadMode::MayRun => EvMilpMode::MayRun,
            MilpLoadMode::MustNotRun => EvMilpMode::MustNotRun,
        };
        Some(EvMilpContext {
            mode: ev_mode,
            a_ev: inputs.a_ev.clone(),
            t_dead_step: inputs.t_ev_dead_step,
            p_max_kw: inputs.p_ev_max_kw,
            p_min_kw: inputs.p_ev_min_kw,
            e_core_kwh: inputs.e_ev_core_kwh,
            e_extra_max_kwh: inputs.e_ev_extra_max_kwh,
            v_extra_eur_kwh: inputs.v_ev_extra_eur_kwh,
        })
    } else {
        None
    };

    // Phase 2: heater carries switching penalty and tier penalty.
    let heat_ctx: Option<HeaterMilpContext> = if inputs.p_heat_full_kw > 0.0 {
        let heat_mode = match inputs.heater_mode {
            MilpLoadMode::MustRun => HeaterMilpMode::MustRun,
            MilpLoadMode::MayRun => HeaterMilpMode::MayRun,
            MilpLoadMode::MustNotRun => HeaterMilpMode::MustNotRun,
        };
        Some(HeaterMilpContext {
            mode: heat_mode,
            t_dead_step: inputs.t_heat_dead_step,
            p_mid_kw: inputs.p_heat_mid_kw,
            p_full_kw: inputs.p_heat_full_kw,
            e_init_kwh: inputs.e_heat_init_kwh,
            e_max_kwh: inputs.e_heat_max_kwh,
            q_dem_kw: inputs.q_heat_dem_kw,
            e_target_kwh: inputs.e_heat_target_kwh,
            lambda_sw_eur: p2w.lambda_heat_sw_eur,
            initial_z_mid: inputs.heat_initial_z_mid,
            initial_z_full: inputs.heat_initial_z_full,
        })
    } else {
        None
    };

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

    // Phase 2: startup/ramp aux vars are declared with real cost values.
    let bat_vars = bat_ctx
        .as_ref()
        .map(|ctx| ctx.declare_vars(n, p2w.c_bat_startup_eur, p2w.c_bat_ramp_eur_kw, &mut vars));
    let ev_vars = ev_ctx
        .as_ref()
        .map(|ctx| ctx.declare_vars(n, p2w.c_ev_startup_eur, p2w.c_ev_ramp_eur_kw, &mut vars));
    let heat_vars = heat_ctx.as_ref().map(|ctx| ctx.declare_vars(n, &mut vars));

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

    let pool = MilpVarPool {
        grid: grid_vars,
        bat: bat_vars,
        ev: ev_vars,
        heater: heat_vars,
        shiftable: shift_vars,
    };

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
    if let Some(v) = &pool.bat {
        // Battery wear only (0.0 startup/ramp in cost cap expression).
        phase1_cap_expr +=
            BatteryMilpContext::objective(v, p1w.c_bat_wear_eur_kwh, 0.0, 0.0, n, dt_h);
    }
    if let (Some(ctx), Some(v)) = (&ev_ctx, &pool.ev) {
        // EV reward only (0.0 startup/ramp in cost cap expression).
        phase1_cap_expr += ctx.objective(v, 0.0, 0.0, p1w.w_services, n);
    }
    if let Some(v) = &pool.heater {
        // m_low heater term manually (bypass ctx.objective to exclude tier/switching).
        for t in 0..n {
            phase1_cap_expr += M_LOW_EUR_PER_KWH * v.s_low[t];
        }
    }
    for (interaction, iv) in active_interactions.iter().zip(iv_list.iter()) {
        phase1_cap_expr += interaction.objective(iv, dt_h);
    }

    // Phase 2 friction objective: startup/ramp/switching/tier; no economic terms.
    let mut friction_obj = Expression::from(0.0);
    if let Some(v) = &pool.bat {
        friction_obj += BatteryMilpContext::objective(
            v,
            0.0,
            p2w.c_bat_startup_eur,
            p2w.c_bat_ramp_eur_kw,
            n,
            dt_h,
        );
    }
    if let (Some(ctx), Some(v)) = (&ev_ctx, &pool.ev) {
        friction_obj += ctx.objective(v, p2w.c_ev_startup_eur, p2w.c_ev_ramp_eur_kw, 0.0, n);
    }
    if let (Some(ctx), Some(v)) = (&heat_ctx, &pool.heater) {
        // Tier + switching (lambda_sw_eur on ctx), no m_low.
        friction_obj += ctx.objective(v, p2w.w_tier_penalty_eur, 0.0, n);
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
        &heat_ctx,
        &bat_ctx,
        &ev_ctx,
        &p_imp,
        &p_exp,
        &u_grid,
        &s_imp_viol,
        &s_exp_viol,
        &active_interactions,
        &iv_list,
        &global,
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
) -> Result<(SolveOutput, f64, f64), Box<dyn std::error::Error>> {
    let phase1_sol = solve_phase1(inputs, p1w)?;
    let c_star = phase1_sol.objective_eur;
    if epsilon == 0.0 {
        return Ok((phase1_sol, c_star, 0.0));
    }
    tracing::debug!(c_star, epsilon, "Phase 2 starting");
    match solve_phase2(inputs, p1w, p2w, c_star, epsilon, &phase1_sol) {
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
