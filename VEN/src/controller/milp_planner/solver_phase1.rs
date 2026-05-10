use chrono::{DateTime, Duration, Utc};
use good_lp::solvers::highs::highs;
use good_lp::{
    constraint, variable, variables, Expression, Solution, SolverModel, Variable,
    WithInitialSolution, WithMipGap, WithTimeLimit,
};
use tracing::warn;
use uuid::Uuid;

use crate::assets::battery::{Battery, BatteryMilpContext};
use crate::assets::ev::{EvCharger, EvMilpContext, EvMilpMode, EvState};
use crate::assets::heater::{Heater, HeaterMilpContext, HeaterMilpMode, HeaterState};
use crate::assets::{AssetState, PvInverter};
use crate::controller::milp_interactions::{
    build_interactions, GlobalMilpInputs, GridMilpVars, MilpVarPool, ShiftableLoadMilpVars,
};
use crate::controller::simulator_port::SimSnapshot;
use crate::entities::asset::PlanTrigger;
use crate::entities::capacity::OadrCapacityState;
use crate::entities::device_session::{BaselineOverride, ShiftableLoad};
use crate::entities::plan::{
    AssetAllocation, CostBreakdown, FlexibilityEnvelope, Plan, PlanSummary, PlanTimeSlot,
    PlanWarning, PlanningHorizon, WarningSeverity,
};
use crate::entities::tariff_snapshot::TariffTimeSeries;
use crate::profile::{PlannerObjective, Profile};

use super::types::*;

/// Run the MILP model and return the optimal schedule.
///
/// Uses per-asset context types from the assets module (`BatteryMilpContext`,
/// `EvMilpContext`, `HeaterMilpContext`) and the `MilpVarPool` / `AssetInteraction`
/// framework from `milp_interactions.rs`. The `MilpInputs` signature is preserved
/// so all existing unit tests continue to compile without modification.

pub(crate) const M_LOW_EUR_PER_KWH: f64 = 10.0;

/// Phase 1: minimise economic cost only. Battery and EV are declared without
/// startup/ramp aux vars (0.0 passed to `declare_vars`). Heater lambda_sw is 0.0.
pub(crate) fn solve_phase1(
    inputs: &MilpInputs,
    p1w: &Phase1Weights,
) -> Result<SolveOutput, Box<dyn std::error::Error>> {
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

    // Phase 1: lambda_sw_eur = 0.0 (switching penalty moves entirely to Phase 2).
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
            lambda_sw_eur: 0.0,
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

    // Phase 1: no startup/ramp aux vars.
    let bat_vars = bat_ctx
        .as_ref()
        .map(|ctx| ctx.declare_vars(n, 0.0, 0.0, &mut vars));
    let ev_vars = ev_ctx
        .as_ref()
        .map(|ctx| ctx.declare_vars(n, 0.0, 0.0, &mut vars));
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
    if let Some(v) = &pool.bat {
        objective += BatteryMilpContext::objective(v, p1w.c_bat_wear_eur_kwh, 0.0, 0.0, n, dt_h);
    }
    if let (Some(ctx), Some(v)) = (&ev_ctx, &pool.ev) {
        objective += ctx.objective(v, 0.0, 0.0, p1w.w_services, n);
    }
    if let (Some(ctx), Some(v)) = (&heat_ctx, &pool.heater) {
        // Phase 1 heater: m_low penalty only (tier=0, lambda_sw=0 on ctx).
        objective += ctx.objective(v, 0.0, M_LOW_EUR_PER_KWH, n);
    }
    for (interaction, iv) in active_interactions.iter().zip(iv_list.iter()) {
        objective += interaction.objective(iv, dt_h);
    }

    let mut model = vars.minimise(&objective).using(highs);
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

    Ok(read_solve_output(&solution, &objective, &pool, inputs, n))
}


/// Helper: add power-balance and per-asset constraints to the model.
#[allow(clippy::too_many_arguments)]
pub(crate) fn add_model_constraints<S: SolverModel>(
    mut model: S,
    inputs: &MilpInputs,
    pool: &MilpVarPool,
    heat_ctx: &Option<HeaterMilpContext>,
    bat_ctx: &Option<BatteryMilpContext>,
    ev_ctx: &Option<EvMilpContext>,
    p_imp: &[Variable],
    p_exp: &[Variable],
    u_grid: &[Variable],
    s_imp_viol: &[Variable],
    s_exp_viol: &[Variable],
    active_interactions: &[&Box<dyn crate::controller::milp_interactions::AssetInteraction>],
    iv_list: &[crate::controller::milp_interactions::InteractionVars],
    global: &GlobalMilpInputs,
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
        let heat_kw: Expression = heat_ctx
            .as_ref()
            .zip(pool.heater.as_ref())
            .map(|(ctx, v)| ctx.power_expr(v, t))
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

    if let (Some(ctx), Some(v)) = (bat_ctx, &pool.bat) {
        for c in ctx.constraints(v, n, global.dt_h) {
            model = model.with(c);
        }
    }
    if let (Some(ctx), Some(v)) = (ev_ctx, &pool.ev) {
        for c in ctx.constraints(v, n, global.dt_h) {
            model = model.with(c);
        }
    }
    if let (Some(ctx), Some(v)) = (heat_ctx, &pool.heater) {
        for c in ctx.constraints(v, n, global.dt_h) {
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
