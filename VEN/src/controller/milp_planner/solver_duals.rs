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
//! This intentionally does not go through `AssetMilpContext::declare_vars_into_pool` (which always
//! declares mode variables as binary): each asset's continuous variables are re-declared here
//! directly from `MilpInputs`' own scalar fields — the same source `build_milp_inputs` used to
//! build the asset contexts in the first place — then the *same* `constraints()`/`objective()`
//! trait methods are called against this locally-built pool. `good_lp::Variable` is just an opaque
//! id, so those methods don't care whether the variable behind it was declared integer or
//! continuous; only whether it's present at the right pool slot with the right cached scalars.
//!
//! Read-only diagnostic: this never feeds back into `p_imp`/`p_exp`/allocations — see
//! `solve_marginal_costs`'s caller (`solve_milp_two_phase`).

use good_lp::solvers::highs::highs;
use good_lp::solvers::{DualValues, SolutionWithDual};
use good_lp::{
    variable, variables, Expression, ProblemVariables, SolverModel, Variable, WithMipGap,
    WithTimeLimit,
};

use crate::controller::milp_interactions::{
    build_interactions, pv_use_tiebreak_expr, shiftable_tiebreak_expr, GlobalMilpInputs,
    GridMilpVars, MilpVarPool, ShiftableLoadMilpVars,
};
use crate::controller::milp_planner::asset_port::{
    BatteryMilpVars, EvMilpVars, HeaterMilpVars, MilpLoadMode,
};
use crate::controller::milp_planner::{AssetKind, AssetMilpContext};

use super::penalty;
use super::solver_phase1::add_model_constraints;
use super::types::*;

fn round_bin(v: f64) -> f64 {
    if v > 0.5 {
        1.0
    } else {
        0.0
    }
}

/// Battery vars with the mode binary (`u_bat`) fixed continuous; power/SoC stay free.
fn declare_fixed_battery_vars(
    inputs: &MilpInputs,
    winning: &SolveOutput,
    n: usize,
    vars: &mut ProblemVariables,
) -> BatteryMilpVars {
    let ch_max = inputs.p_bat_ch_max_kw.unwrap_or(0.0);
    let dis_max = inputs.p_bat_dis_max_kw.unwrap_or(0.0);
    let e_min = inputs.e_bat_min_kwh.unwrap_or(0.0);
    let e_max = inputs.e_bat_max_kwh.unwrap_or(0.0);
    let e_init = inputs.e_bat_init_kwh.unwrap_or(0.0);

    let p_ch = (0..n)
        .map(|_| vars.add(variable().min(0.0).max(ch_max)))
        .collect();
    let p_dis = (0..n)
        .map(|_| vars.add(variable().min(0.0).max(dis_max)))
        .collect();
    let u_bat = (0..n)
        .map(|t| {
            let v = round_bin(if winning.p_bat_ch_kw[t] > 1e-6 {
                1.0
            } else {
                0.0
            });
            vars.add(variable().min(v).max(v))
        })
        .collect();
    let e_bat = (0..=n)
        .map(|i| {
            if i == 0 {
                vars.add(variable().min(e_init).max(e_init))
            } else {
                vars.add(variable().min(e_min).max(e_max))
            }
        })
        .collect();
    BatteryMilpVars {
        p_ch,
        p_dis,
        u_bat,
        e_bat,
        z_active: vec![],
        delta_active: vec![],
        delta_ramp: vec![],
        dis_max_kw: dis_max,
    }
}

/// EV vars with `z_ev_on`/`z_ev_core` fixed continuous; power/extra-energy stay free.
fn declare_fixed_ev_vars(
    inputs: &MilpInputs,
    winning: &SolveOutput,
    n: usize,
    vars: &mut ProblemVariables,
) -> EvMilpVars {
    let must_not = inputs.ev_mode == MilpLoadMode::MustNotRun;
    let p_max = if must_not { 0.0 } else { inputs.p_ev_max_kw };
    let e_extra_max = if must_not {
        0.0
    } else {
        inputs.e_ev_extra_max_kwh
    };

    let p_ev = (0..n)
        .map(|_| vars.add(variable().min(0.0).max(p_max)))
        .collect();
    let z_ev_on = (0..n)
        .map(|t| {
            let v = round_bin(winning.z_ev_on[t]);
            vars.add(variable().min(v).max(v))
        })
        .collect();
    let z_ev_core = {
        let v = round_bin(winning.z_ev_core);
        vars.add(variable().min(v).max(v))
    };
    let e_ev_extra = vars.add(variable().min(0.0).max(e_extra_max));
    EvMilpVars {
        p_ev,
        z_ev_on,
        z_ev_core,
        e_ev_extra,
        delta_ev: vec![],
        delta_ev_ramp: vec![],
        p_min_kw: inputs.p_ev_min_kw,
    }
}

/// Heater vars with the tier binaries (`z_heat_mid`/`z_heat_full`/`z_heat_ready`) fixed
/// continuous; tank energy/switching stay free.
fn declare_fixed_heater_vars(
    inputs: &MilpInputs,
    winning: &SolveOutput,
    n: usize,
    vars: &mut ProblemVariables,
) -> HeaterMilpVars {
    let z_heat_mid = (0..n)
        .map(|t| {
            let v = round_bin(winning.z_heat_mid[t]);
            vars.add(variable().min(v).max(v))
        })
        .collect();
    let z_heat_full = (0..n)
        .map(|t| {
            let v = round_bin(winning.z_heat_full[t]);
            vars.add(variable().min(v).max(v))
        })
        .collect();
    let z_heat_ready = {
        let v = round_bin(winning.z_heat_ready);
        vars.add(variable().min(v).max(v))
    };
    let e_lo = -inputs.e_heat_max_kwh.max(1.0);
    let e_hi = inputs.e_heat_max_kwh.max(1.0);
    let e_tank = (0..n)
        .map(|_| vars.add(variable().min(e_lo).max(e_hi)))
        .collect();
    let s_low = (0..n).map(|_| vars.add(variable().min(0.0))).collect();
    let sw = (0..n).map(|_| vars.add(variable().min(0.0))).collect();
    HeaterMilpVars {
        z_heat_mid,
        z_heat_full,
        z_heat_ready,
        e_tank,
        s_low,
        sw,
        p_mid_kw: inputs.p_heat_mid_kw,
        p_full_kw: inputs.p_heat_full_kw,
    }
}

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
            let v = round_bin(if winning.p_imp_kw[t] > 1e-6 { 1.0 } else { 0.0 });
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

    let shift_vars: Vec<ShiftableLoadMilpVars> = inputs
        .shiftable_loads
        .iter()
        .enumerate()
        .map(|(s, sl)| {
            let y_shift = sl
                .valid_start_slots
                .iter()
                .map(|&j| {
                    let active = round_bin(if winning.p_shiftable_kw[s][j] > 0.01 {
                        1.0
                    } else {
                        0.0
                    });
                    vars.add(variable().min(active).max(active))
                })
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

    // WP6.3 (BL-09): declared fresh (not fixed to `winning`, like s_imp_viol above) —
    // this is a free continuous slack, not a mode decision, so it needs no rounding fix.
    let penalty_vars =
        penalty::declare_penalty_vars(&inputs.penalty_rules, &inputs.cum_s, &mut vars);

    for ctx in asset_contexts {
        match ctx.asset_kind() {
            AssetKind::Battery => {
                pool.bat = Some(declare_fixed_battery_vars(inputs, winning, n, &mut vars));
            }
            AssetKind::Ev => {
                pool.ev = Some(declare_fixed_ev_vars(inputs, winning, n, &mut vars));
            }
            AssetKind::Heater => {
                pool.heater = Some(declare_fixed_heater_vars(inputs, winning, n, &mut vars));
            }
        }
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
