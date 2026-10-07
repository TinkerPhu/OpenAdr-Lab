//! The model skeleton the planner's solves share: grid variables, penalty slacks, asset variables,
//! cross-asset interactions, the economic cost, and the constraints that tie them together.
//!
//! Phase 1, phase 2 and the marginal-cost pass solve the *same* economic model; they differ in how
//! decisions are declared (see [`Declaration`]) and in what is minimised on top. Each used to build
//! this skeleton by hand, with comments promising to "mirror phase 1 exactly", and the copies drifted
//! (R-98). It is built here once, so a new grid variable or objective term is added in one place and
//! reaches every solve.
//!
//! The order in which variables, expressions and constraints are created is part of the contract:
//! `good_lp` keeps coefficients in a hash map, so what HiGHS receives is a function of that order.
//! `tests/model_fingerprint.rs` records the model each solve sends and compares it with a golden.

use good_lp::{
    constraint, variable, variables, Constraint, Expression, ProblemVariables, Solution,
    SolverModel, Variable,
};

use super::asset_port::{
    pinned_binary, BatteryMilpContext, EvMilpContext, HeaterMilpContext, WinningModeDecisions,
};
use super::penalty::{self, PenaltyRuleVars};
use super::types::*;
use crate::controller::milp_interactions::{
    build_interactions, pv_use_tiebreak_expr, shiftable_tiebreak_expr, AssetInteraction,
    GlobalMilpInputs, GridMilpVars, InteractionVars, MilpVarPool,
};
use crate::controller::milp_planner::{AssetKind, AssetMilpContext};

/// How a solve declares its decisions. The one place the three solves differ in what they build.
pub(super) enum Declaration<'a> {
    /// Phase 1: every decision is free (the grid direction a binary) and each asset is declared
    /// without startup/ramp auxiliaries, i.e. with both costs `0.0`.
    Phase1,
    /// Phase 2: every decision is free, and battery and EV carry their startup/ramp auxiliaries
    /// with the real costs from the weights, because phase 2 minimises that friction.
    Phase2(&'a Phase2Weights),
    /// The marginal-cost pass: every mode decision (grid direction, battery direction, EV on/off,
    /// heater stage, shiftable start) is fixed to the winning solution's value as a *continuous*
    /// variable, because HiGHS returns no duals for a model with any integer column.
    Pinned(&'a SolveOutput),
}

impl Declaration<'_> {
    /// One slot's grid import/export exclusion variable.
    fn declare_u_grid(&self, t: usize, vars: &mut ProblemVariables) -> Variable {
        match self {
            Declaration::Phase1 | Declaration::Phase2(_) => vars.add(variable().binary()),
            Declaration::Pinned(winning) => {
                // u_grid is a mode decision like the asset binaries: fixed continuous, not
                // `.binary()`, for the reason given on `Declaration::Pinned`.
                let v = pinned_binary(Some(if winning.p_imp_kw[t] > 1e-6 { 1.0 } else { 0.0 }));
                vars.add(variable().min(v).max(v))
            }
        }
    }

    fn declare_asset(
        &self,
        ctx: &dyn AssetMilpContext,
        n: usize,
        vars: &mut ProblemVariables,
        pool: &mut MilpVarPool,
    ) {
        match self {
            Declaration::Phase1 => ctx.declare_vars_into_pool(n, 0.0, 0.0, vars, pool),
            Declaration::Phase2(p2w) => {
                let (c_startup_eur, c_ramp_eur_kw) = match ctx.asset_kind() {
                    AssetKind::Battery => (p2w.c_bat_startup_eur, p2w.c_bat_ramp_eur_kw),
                    AssetKind::Ev => (p2w.c_ev_startup_eur, p2w.c_ev_ramp_eur_kw),
                    AssetKind::Heater | AssetKind::ShiftableLoad => (0.0, 0.0),
                };
                ctx.declare_vars_into_pool(n, c_startup_eur, c_ramp_eur_kw, vars, pool)
            }
            // R-98: through the same function the plan used, with the winning mode decisions
            // pinned, so the priced model is the planned one.
            Declaration::Pinned(winning) => {
                ctx.declare_pinned_vars_into_pool(n, &winning.mode_decisions, vars, pool)
            }
        }
    }
}

struct ActiveInteraction {
    interaction: Box<dyn AssetInteraction>,
    vars: InteractionVars,
}

/// Everything declared before a solve adds its own objective: handles for every variable, the
/// grid-level inputs, the penalty slacks and the interactions that apply to this set of assets.
pub(super) struct ModelSkeleton {
    pub(super) global: GlobalMilpInputs,
    pub(super) pool: MilpVarPool,
    pub(super) penalty_vars: Vec<PenaltyRuleVars>,
    interactions: Vec<ActiveInteraction>,
}

impl ModelSkeleton {
    /// Declare the skeleton's variables, in this order: grid variables, penalty slacks, each
    /// asset's variables, then the variables of every interaction that applies. Returns
    /// `ProblemVariables` apart from the handles so the caller can `minimise` it.
    pub(super) fn declare(
        inputs: &MilpInputs,
        p1w: &Phase1Weights,
        asset_contexts: &[Box<dyn AssetMilpContext>],
        declaration: Declaration<'_>,
    ) -> (ProblemVariables, Self) {
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
        let u_grid: Vec<Variable> = (0..n)
            .map(|t| declaration.declare_u_grid(t, &mut vars))
            .collect();
        let s_imp_viol: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();
        let s_exp_viol: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();
        let p_pv_used: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();

        let mut pool = MilpVarPool {
            grid: GridMilpVars {
                p_imp,
                p_exp,
                u_grid,
                s_imp_viol,
                s_exp_viol,
                p_pv_used,
            },
            bat: None,
            ev: None,
            heater: None,
            // Populated generically below by each ShiftableLoadMilpContext's own declaration,
            // the same loop that declares Battery/EV/Heater's variables.
            shiftable: Vec::new(),
        };

        // WP6.3 (BL-09): one slack per window per active penalty rule, a free continuous slack
        // rather than a mode decision, so it is never pinned. Empty `penalty_rules` declares
        // nothing.
        let penalty_vars =
            penalty::declare_penalty_vars(&inputs.penalty_rules, &inputs.cum_s, &mut vars);

        for ctx in asset_contexts {
            declaration.declare_asset(ctx.as_ref(), n, &mut vars, &mut pool);
        }

        let mut interactions = Vec::new();
        for interaction in
            build_interactions(p1w.c_bat_ev_coexist_eur_kwh, p1w.c_ctrl_imp_malus_eur_kwh)
        {
            if interaction.applicable(&pool) {
                let iv = interaction.declare_vars(&pool, &global, &mut vars);
                interactions.push(ActiveInteraction {
                    interaction,
                    vars: iv,
                });
            }
        }

        (
            vars,
            Self {
                global,
                pool,
                penalty_vars,
                interactions,
            },
        )
    }

    /// Phase 1's economic cost: energy, emissions, grid and violation terms, the penalty slacks,
    /// each asset's own cost, the interactions, and the two deterministic tie-breaks.
    ///
    /// This single expression is phase 1's objective, phase 2's cost cap and the marginal-cost
    /// pass's objective. They must be one expression: `c_star` (phase 1's optimum) has to equal the
    /// cap term for term, or phase 2 becomes infeasible outright, and the shadow price has to
    /// reflect the objective that was actually optimised.
    pub(super) fn cost_expr(
        &self,
        inputs: &MilpInputs,
        p1w: &Phase1Weights,
        asset_contexts: &[Box<dyn AssetMilpContext>],
    ) -> Expression {
        let n = inputs.n;
        let grid = &self.pool.grid;
        let mut objective = Expression::from(0.0);
        for t in 0..n {
            objective += (p1w.w_energy * inputs.dt_h[t] * inputs.c_imp_eur_kwh[t]) * grid.p_imp[t];
            objective += -(p1w.w_energy * inputs.dt_h[t] * inputs.c_exp_eur_kwh[t]) * grid.p_exp[t];
            objective += (p1w.w_ghg * inputs.dt_h[t] * inputs.g_imp_kgco2_kwh[t]) * grid.p_imp[t];
            objective += (p1w.w_grid * inputs.dt_h[t]) * grid.p_imp[t];
            objective += (p1w.w_grid * inputs.dt_h[t]) * grid.p_exp[t];
            objective += (p1w.w_import * inputs.dt_h[t]) * grid.p_imp[t];
            objective +=
                (p1w.w_viol * inputs.pen_imp_eur_kwh * inputs.dt_h[t]) * grid.s_imp_viol[t];
            objective +=
                (p1w.w_viol * inputs.pen_exp_eur_kwh * inputs.dt_h[t]) * grid.s_exp_viol[t];
        }
        // WP6.3 (BL-09): peak-demand penalty slack cost, once per window (not per slot).
        objective += penalty::penalty_objective(&self.penalty_vars);
        // Each asset's contribution in phase 1 convention: a zero startup cost selects the
        // "phase 1" branch inside an asset's `objective` (battery: wear only; EV: service reward
        // only; heater: below-minimum penalty only; shiftable load: no economic term).
        for ctx in asset_contexts {
            let wear_eur_kwh = match ctx.asset_kind() {
                AssetKind::Battery => p1w.c_bat_wear_eur_kwh,
                AssetKind::Ev | AssetKind::Heater | AssetKind::ShiftableLoad => 0.0,
            };
            objective += ctx.objective(&self.pool, n, &inputs.dt_h, wear_eur_kwh, 0.0, 0.0);
        }
        for active in &self.interactions {
            objective += active.interaction.objective(&active.vars, &inputs.dt_h);
        }
        // Deterministic earliest-start tie-break for shiftable loads (see
        // SHIFT_TIEBREAK_EUR_PER_SLOT for the rationale).
        objective += shiftable_tiebreak_expr(&self.pool.shiftable);
        // Full-PV-utilization tie-break (see PV_USE_TIEBREAK_EUR_PER_KWH for the rationale).
        objective += pv_use_tiebreak_expr(&self.pool.grid, &inputs.dt_h);
        objective
    }

    /// Add the power balance and grid bounds per slot, then every asset's, interaction's and
    /// penalty rule's constraints. Returns the model and the per-slot power-balance references,
    /// which the marginal-cost pass reads shadow prices off (the solve phases discard them).
    pub(super) fn add_constraints<S: SolverModel>(
        &self,
        mut model: S,
        inputs: &MilpInputs,
        asset_contexts: &[Box<dyn AssetMilpContext>],
    ) -> (S, Vec<good_lp::constraint::ConstraintReference>) {
        let n = inputs.n;
        let pool = &self.pool;
        let grid = &pool.grid;
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
                grid.p_imp[t] + grid.p_pv_used[t] + bat_dis
                    == inputs.p_base_kw[t] + ev_kw + heat_kw + shift_kw + bat_ch + grid.p_exp[t]
            );
            probe!(constraint, &power_balance);
            power_balance_refs.push(model.add_constraint(power_balance));
            model = with_constraint(model, constraint!(grid.p_pv_used[t] <= inputs.p_pv_kw[t]));
            model = with_constraint(
                model,
                constraint!(grid.p_imp[t] <= inputs.p_imp_max_phys_kw[t] * grid.u_grid[t]),
            );
            model = with_constraint(
                model,
                constraint!(grid.p_exp[t] <= inputs.p_exp_max_phys_kw[t] * (1.0 - grid.u_grid[t])),
            );
            model = with_constraint(
                model,
                constraint!(grid.p_imp[t] <= inputs.p_imp_max_cont_kw[t] + grid.s_imp_viol[t]),
            );
            model = with_constraint(
                model,
                constraint!(grid.p_exp[t] <= inputs.p_exp_max_cont_kw[t] + grid.s_exp_viol[t]),
            );
        }

        for ctx in asset_contexts {
            for c in ctx.constraints(pool, n, &self.global.dt_h) {
                model = with_constraint(model, c);
            }
        }

        for active in &self.interactions {
            for c in active
                .interaction
                .constraints(pool, &active.vars, &self.global)
            {
                model = with_constraint(model, c);
            }
        }

        // WP6.3 (BL-09): p_imp[t] <= threshold_kw + s_penalty[window_of(t)] for each active rule.
        for c in penalty::penalty_constraints(&grid.p_imp, &inputs.cum_s, &self.penalty_vars) {
            model = with_constraint(model, c);
        }
        (model, power_balance_refs)
    }

    /// Extract a `SolveOutput` from a solved `good_lp::Solution`.
    pub(super) fn read_output<S: Solution>(
        &self,
        solution: &S,
        objective: &Expression,
        inputs: &MilpInputs,
    ) -> SolveOutput {
        let n = inputs.n;
        let pool = &self.pool;
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

        let mode_decisions = WinningModeDecisions {
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
            s_penalty_kw: penalty::read_penalty_solution(solution, &self.penalty_vars),
        }
    }
}

/// `model.with(c)`, recording `c` first when a test is capturing the model (`probe!`).
pub(super) fn with_constraint<S: SolverModel>(model: S, c: Constraint) -> S {
    probe!(constraint, &c);
    model.with(c)
}
