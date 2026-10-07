use good_lp::solvers::highs::highs;
use good_lp::{constraint, Expression, Solution, SolverModel, Variable, WithMipGap, WithTimeLimit};

use super::asset_port::{BatteryMilpContext, EvMilpContext, HeaterMilpContext};
use crate::controller::milp_interactions::{GlobalMilpInputs, MilpVarPool};
use crate::controller::milp_planner::AssetMilpContext;

use super::model_skeleton::{with_constraint, Declaration, ModelSkeleton};
use super::penalty::{self, PenaltyRuleVars};
use super::types::*;

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

    Ok(read_solve_output(
        &solution,
        &objective,
        &skeleton.pool,
        inputs,
        inputs.n,
        &skeleton.penalty_vars,
    ))
}

/// Helper: add power-balance and per-asset constraints to the model.
/// Returns the model plus the per-slot power-balance `ConstraintReference`s
/// (needed by `solver_duals.rs` to read the shadow price off each row;
/// unused by the two solve-phase callers, which discard it).
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
    active_interactions: &[&dyn crate::controller::milp_interactions::AssetInteraction],
    iv_list: &[crate::controller::milp_interactions::InteractionVars],
    global: &GlobalMilpInputs,
    asset_contexts: &[Box<dyn AssetMilpContext>],
    n: usize,
    penalty_vars: &[PenaltyRuleVars],
) -> (S, Vec<good_lp::constraint::ConstraintReference>) {
    let mut power_balance_refs = Vec::with_capacity(n);
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
        // Heater power balance from the cached per-stage power in HeaterMilpVars.
        let heat_kw: Expression = pool
            .heater
            .as_ref()
            .map(|v| v.p_step_kw * v.y_heat[t])
            .unwrap_or_else(|| Expression::from(0.0));

        let power_balance = constraint!(
            p_imp[t] + pool.grid.p_pv_used[t] + bat_dis
                == inputs.p_base_kw[t] + ev_kw + heat_kw + shift_kw + bat_ch + p_exp[t]
        );
        probe!(constraint, &power_balance);
        let power_balance_ref = model.add_constraint(power_balance);
        power_balance_refs.push(power_balance_ref);
        model = with_constraint(
            model,
            constraint!(pool.grid.p_pv_used[t] <= inputs.p_pv_kw[t]),
        );
        model = with_constraint(
            model,
            constraint!(p_imp[t] <= inputs.p_imp_max_phys_kw[t] * u_grid[t]),
        );
        model = with_constraint(
            model,
            constraint!(p_exp[t] <= inputs.p_exp_max_phys_kw[t] * (1.0 - u_grid[t])),
        );
        model = with_constraint(
            model,
            constraint!(p_imp[t] <= inputs.p_imp_max_cont_kw[t] + s_imp_viol[t]),
        );
        model = with_constraint(
            model,
            constraint!(p_exp[t] <= inputs.p_exp_max_cont_kw[t] + s_exp_viol[t]),
        );
    }

    for ctx in asset_contexts {
        for c in ctx.constraints(pool, n, &global.dt_h) {
            model = with_constraint(model, c);
        }
    }

    for (interaction, iv) in active_interactions.iter().zip(iv_list.iter()) {
        for c in interaction.constraints(pool, iv, global) {
            model = with_constraint(model, c);
        }
    }

    // WP6.3 (BL-09): p_imp[t] <= threshold_kw + s_penalty[window_of(t)] for each active rule.
    for c in penalty::penalty_constraints(p_imp, &inputs.cum_s, penalty_vars) {
        model = with_constraint(model, c);
    }
    (model, power_balance_refs)
}

/// Extract a `SolveOutput` from a solved `good_lp::Solution`.
pub(crate) fn read_solve_output<S: Solution>(
    solution: &S,
    objective: &Expression,
    pool: &MilpVarPool,
    inputs: &MilpInputs,
    n: usize,
    penalty_vars: &[PenaltyRuleVars],
) -> SolveOutput {
    let p_imp_ref = &pool.grid.p_imp;
    let p_exp_ref = &pool.grid.p_exp;
    let s_imp_ref = &pool.grid.s_imp_viol;
    let s_exp_ref = &pool.grid.s_exp_viol;
    let p_pv_used_ref = &pool.grid.p_pv_used;

    let (bat_ch_kw, bat_dis_kw, e_bat_kwh) = if let Some(v) = &pool.bat {
        let sol = BatteryMilpContext::read_solution(solution, v, n);
        (sol.p_ch_kw, sol.p_dis_kw, sol.e_kwh)
    } else {
        (vec![0.0; n], vec![0.0; n], vec![0.0; n + 1])
    };

    let (ev_kw_out, soc_ev_out, ev_shortfall_out, z_ev_on_out, e_ev_extra_out, e_seg_out) =
        if let Some(v) = &pool.ev {
            let sol = EvMilpContext::read_solution(solution, v, n);
            (
                sol.p_ev_kw,
                sol.soc_ev,
                sol.shortfall_kwh,
                sol.z_ev_on,
                sol.e_ev_extra_kwh,
                sol.e_seg_kwh,
            )
        } else {
            (
                vec![0.0; n],
                // No EV in the model: a flat curve at the live reading, so a
                // consumer never has to special-case a missing series.
                vec![inputs.soc_ev_init.unwrap_or(0.0); n + 1],
                Vec::new(),
                vec![0.0; n],
                0.0,
                0.0,
            )
        };

    let (y_heat_out, z_heat_ready_out, e_heat_tank_out) = if let Some(v) = &pool.heater {
        let sol = HeaterMilpContext::read_solution(solution, v, n);
        (sol.y_heat, sol.z_heat_ready, sol.e_tank_kwh)
    } else {
        (vec![0.0; n], 0.0, vec![])
    };

    let mut p_shiftable_kw = vec![vec![0.0; n]; inputs.shiftable_loads.len()];
    for (row, sv) in p_shiftable_kw.iter_mut().zip(pool.shiftable.iter()) {
        for (t, slot_kw) in row.iter_mut().enumerate() {
            for (ji, &j) in sv.valid_start_slots.iter().enumerate() {
                if t >= j && t < j + sv.duration_slots {
                    *slot_kw += sv.power_kw * solution.value(sv.y_shift[ji]);
                }
            }
        }
    }

    let mode_decisions = super::asset_port::WinningModeDecisions {
        u_bat: pool
            .bat
            .as_ref()
            .map(|v| v.u_bat.iter().map(|&u| solution.value(u)).collect())
            .unwrap_or_default(),
        z_ev_on: z_ev_on_out.clone(),
        y_heat: y_heat_out.clone(),
        z_heat_ready: z_heat_ready_out,
        y_shift: pool
            .shiftable
            .iter()
            .map(|sv| {
                let row = sv.y_shift.iter().map(|&y| solution.value(y)).collect();
                (sv.asset_id.clone(), row)
            })
            .collect(),
    };

    SolveOutput {
        status: solution.status(),
        objective_eur: solution.eval(objective),
        p_imp_kw: (0..n).map(|t| solution.value(p_imp_ref[t])).collect(),
        p_exp_kw: (0..n).map(|t| solution.value(p_exp_ref[t])).collect(),
        p_pv_used_kw: (0..n).map(|t| solution.value(p_pv_used_ref[t])).collect(),
        p_bat_ch_kw: bat_ch_kw,
        p_bat_dis_kw: bat_dis_kw,
        p_ev_kw: ev_kw_out,
        soc_ev: soc_ev_out,
        ev_shortfall_kwh: ev_shortfall_out,
        y_heat: y_heat_out,
        e_bat_kwh,
        s_imp_viol_kw: (0..n).map(|t| solution.value(s_imp_ref[t])).collect(),
        s_exp_viol_kw: (0..n).map(|t| solution.value(s_exp_ref[t])).collect(),
        z_ev_on: z_ev_on_out,
        e_ev_extra: e_ev_extra_out,
        e_seg_kwh: e_seg_out,
        z_heat_ready: z_heat_ready_out,
        e_heat_tank_kwh: e_heat_tank_out,
        p_shiftable_kw,
        mode_decisions,
        s_penalty_kw: penalty::read_penalty_solution(solution, penalty_vars),
    }
}
