use good_lp::solvers::highs::highs;
use good_lp::{
    constraint, variable, variables, Expression, Solution, SolverModel, Variable, WithMipGap, WithTimeLimit,
};

use crate::controller::milp_interactions::{
    build_interactions, GlobalMilpInputs, GridMilpVars, MilpVarPool, ShiftableLoadMilpVars,
};
use crate::controller::milp_planner::{AssetKind, AssetMilpContext};
use super::asset_port::{BatteryMilpContext, EvMilpContext, HeaterMilpContext};

use super::types::*;


/// Phase 1: minimise economic cost only. Battery and EV declared without startup/ramp aux vars.
/// Heater objective uses m_low penalty only (lambda_sw=0 via c_startup_eur=0.0 convention).
pub(crate) fn solve_phase1(
    inputs: &MilpInputs,
    p1w: &Phase1Weights,
    asset_contexts: &[Box<dyn AssetMilpContext>],
    timeout_s: f64,
) -> Result<SolveOutput, Box<dyn std::error::Error>> {
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

    // Phase 1: startup/ramp = 0.0 for all assets.
    for ctx in asset_contexts {
        ctx.declare_vars_into_pool(n, 0.0, 0.0, &mut vars, &mut pool);
    }

    let interactions = build_interactions(p1w.c_bat_ev_coexist_eur_kwh, p1w.c_ctrl_imp_malus_eur_kwh);
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

    // Phase 1 objective: economic + m_low; no startup/ramp/switching/tier friction.
    let mut objective = Expression::from(0.0);
    for t in 0..n {
        objective += (p1w.w_energy * dt_h * inputs.c_imp_eur_kwh[t]) * p_imp[t];
        objective += -(p1w.w_energy * dt_h * inputs.c_exp_eur_kwh[t]) * p_exp[t];
        objective += (p1w.w_ghg * dt_h * inputs.g_imp_kgco2_kwh[t]) * p_imp[t];
        objective += (p1w.w_grid * dt_h) * p_imp[t];
        objective += (p1w.w_grid * dt_h) * p_exp[t];
        objective += (p1w.w_import * dt_h) * p_imp[t];
        objective += (p1w.w_viol * inputs.pen_imp_eur_kwh * dt_h) * s_imp_viol[t];
        objective += (p1w.w_viol * inputs.pen_exp_eur_kwh * dt_h) * s_exp_viol[t];
    }
    // Asset objective contributions — Phase 1: c_startup=0.0, c_ramp=0.0.
    // Battery: wear only. EV: service reward only. Heater: m_low penalty only.
    for ctx in asset_contexts {
        match ctx.asset_kind() {
            AssetKind::Battery => {
                objective += ctx.objective(&pool, n, dt_h, p1w.c_bat_wear_eur_kwh, 0.0, 0.0);
            }
            AssetKind::Ev => {
                objective += ctx.objective(&pool, n, dt_h, 0.0, 0.0, 0.0);
            }
            AssetKind::Heater => {
                // c_startup=0.0 signals Phase 1 → m_low penalty, no tier/switching.
                objective += ctx.objective(&pool, n, dt_h, 0.0, 0.0, 0.0);
            }
        }
    }
    for (interaction, iv) in active_interactions.iter().zip(iv_list.iter()) {
        objective += interaction.objective(iv, dt_h);
    }

    let mut model = vars.minimise(&objective).using(highs);
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
    model = model.with_time_limit(timeout_s);
    model = model.with_mip_gap(0.02)?;
    let solution = model.solve()?;

    Ok(read_solve_output(&solution, &objective, &pool, inputs, n))
}


/// Helper: add power-balance and per-asset constraints to the model.
#[allow(clippy::too_many_arguments)]
pub(crate) fn add_model_constraints<S: SolverModel>(
    mut model: S,
    inputs: &MilpInputs,
    pool: &MilpVarPool,
    p_imp: &[Variable],
    p_exp: &[Variable],
    u_grid: &[Variable],
    s_imp_viol: &[Variable],
    s_exp_viol: &[Variable],
    active_interactions: &[&Box<dyn crate::controller::milp_interactions::AssetInteraction>],
    iv_list: &[crate::controller::milp_interactions::InteractionVars],
    global: &GlobalMilpInputs,
    asset_contexts: &[Box<dyn AssetMilpContext>],
    n: usize,
) -> S {
    for t in 0..n {
        let mut shift_kw = Expression::from(0.0);
        for sv in &pool.shiftable {
            for (ji, &j) in sv.valid_start_slots.iter().enumerate() {
                if t >= j && t < j + sv.duration_slots {
                    shift_kw += sv.power_kw * sv.y_shift[ji];
                }
            }
        }

        let bat_dis: Expression = pool
            .bat
            .as_ref()
            .map(|v| Expression::from(v.p_dis[t]))
            .unwrap_or_else(|| Expression::from(0.0));
        let bat_ch: Expression = pool
            .bat
            .as_ref()
            .map(|v| Expression::from(v.p_ch[t]))
            .unwrap_or_else(|| Expression::from(0.0));
        let ev_kw: Expression = pool
            .ev
            .as_ref()
            .map(|v| Expression::from(v.p_ev[t]))
            .unwrap_or_else(|| Expression::from(0.0));
        // Heater power balance from cached p_mid_kw/p_full_kw in HeaterMilpVars.
        let heat_kw: Expression = pool
            .heater
            .as_ref()
            .map(|v| {
                Expression::from(v.p_mid_kw * v.z_heat_mid[t])
                    + Expression::from(v.p_full_kw * v.z_heat_full[t])
            })
            .unwrap_or_else(|| Expression::from(0.0));

        model = model.with(constraint!(
            p_imp[t] + inputs.p_pv_kw[t] + bat_dis
                == inputs.p_base_kw[t] + ev_kw + heat_kw + shift_kw + bat_ch + p_exp[t]
        ));
        model = model.with(constraint!(
            p_imp[t] <= inputs.p_imp_max_phys_kw[t] * u_grid[t]
        ));
        model = model.with(constraint!(
            p_exp[t] <= inputs.p_exp_max_phys_kw[t] * (1.0 - u_grid[t])
        ));
        model = model.with(constraint!(
            p_imp[t] <= inputs.p_imp_max_cont_kw[t] + s_imp_viol[t]
        ));
        model = model.with(constraint!(
            p_exp[t] <= inputs.p_exp_max_cont_kw[t] + s_exp_viol[t]
        ));
    }

    for ctx in asset_contexts {
        for c in ctx.constraints(pool, n, global.dt_h) {
            model = model.with(c);
        }
    }

    for sv in &pool.shiftable {
        let mut sum_y = Expression::from(0.0);
        for &y in &sv.y_shift {
            sum_y += y;
        }
        model = model.with(constraint!(sum_y == 1.0));
    }

    for (interaction, iv) in active_interactions.iter().zip(iv_list.iter()) {
        for c in interaction.constraints(pool, iv, global) {
            model = model.with(c);
        }
    }
    model
}

/// Extract a `SolveOutput` from a solved `good_lp::Solution`.
pub(crate) fn read_solve_output<S: Solution>(
    solution: &S,
    objective: &Expression,
    pool: &MilpVarPool,
    inputs: &MilpInputs,
    n: usize,
) -> SolveOutput {
    let p_imp_ref = &pool.grid.p_imp;
    let p_exp_ref = &pool.grid.p_exp;
    let s_imp_ref = &pool.grid.s_imp_viol;
    let s_exp_ref = &pool.grid.s_exp_viol;

    let (bat_ch_kw, bat_dis_kw, e_bat_kwh) = if let Some(v) = &pool.bat {
        let sol = BatteryMilpContext::read_solution(solution, v, n);
        (sol.p_ch_kw, sol.p_dis_kw, sol.e_kwh)
    } else {
        (vec![0.0; n], vec![0.0; n], vec![0.0; n + 1])
    };

    let (ev_kw_out, z_ev_on_out, e_ev_extra_out, z_ev_core_out) = if let Some(v) = &pool.ev {
        let sol = EvMilpContext::read_solution(solution, v, n);
        (sol.p_ev_kw, sol.z_ev_on, sol.e_ev_extra_kwh, sol.z_ev_core)
    } else {
        (vec![0.0; n], vec![0.0; n], 0.0, 0.0)
    };

    let (z_heat_mid_out, z_heat_full_out, z_heat_ready_out, e_heat_tank_out) =
        if let Some(v) = &pool.heater {
            let sol = HeaterMilpContext::read_solution(solution, v, n);
            (
                sol.z_heat_mid,
                sol.z_heat_full,
                sol.z_heat_ready,
                sol.e_tank_kwh,
            )
        } else {
            (vec![0.0; n], vec![0.0; n], 0.0, vec![])
        };

    let mut p_shiftable_kw = vec![vec![0.0; n]; inputs.shiftable_loads.len()];
    for (s, sv) in pool.shiftable.iter().enumerate() {
        for t in 0..n {
            for (ji, &j) in sv.valid_start_slots.iter().enumerate() {
                if t >= j && t < j + sv.duration_slots {
                    p_shiftable_kw[s][t] += sv.power_kw * solution.value(sv.y_shift[ji]);
                }
            }
        }
    }

    SolveOutput {
        objective_eur: solution.eval(objective),
        p_imp_kw: (0..n).map(|t| solution.value(p_imp_ref[t])).collect(),
        p_exp_kw: (0..n).map(|t| solution.value(p_exp_ref[t])).collect(),
        p_bat_ch_kw: bat_ch_kw,
        p_bat_dis_kw: bat_dis_kw,
        p_ev_kw: ev_kw_out,
        z_heat_mid: z_heat_mid_out,
        z_heat_full: z_heat_full_out,
        e_bat_kwh,
        s_imp_viol_kw: (0..n).map(|t| solution.value(s_imp_ref[t])).collect(),
        s_exp_viol_kw: (0..n).map(|t| solution.value(s_exp_ref[t])).collect(),
        z_ev_on: z_ev_on_out,
        e_ev_extra: e_ev_extra_out,
        z_ev_core: z_ev_core_out,
        z_heat_ready: z_heat_ready_out,
        e_heat_tank_kwh: e_heat_tank_out,
        p_shiftable_kw,
    }
}
