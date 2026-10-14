//! The model skeleton the planner's solves share: grid variables, penalty slacks, asset variables,
//! cross-asset interactions, the economic cost, and the constraints that tie them together.
//!
//! Phase 1, phase 2 and the marginal-cost pass solve the *same* economic model; they differ in how
//! decisions are declared (see [`Declaration`]) and in what is minimised on top. Each used to build
//! this skeleton by hand, with comments promising to "mirror phase 1 exactly", and the copies drifted
//! (R-98). It is built here once, so a new grid variable or objective term is added in one place and
//! reaches every solve.
//!
//! Nothing here knows an asset's kind: each asset declares its own variables (`DeclaredAsset`),
//! adds its own net power to the balance, contributes its own cost and friction, and reads back
//! its own solution.
//!
//! The order in which variables, expressions and constraints are created is part of the contract:
//! `good_lp` keeps coefficients in a hash map, so what HiGHS receives is a function of that order.
//! `tests/model_fingerprint.rs` records the model each solve sends and compares it with a golden.

use good_lp::{
    constraint, variable, variables, Constraint, Expression, ProblemVariables, Solution,
    SolverModel, Variable,
};

use super::asset_port::pinned_binary;
use super::penalty::{self, PenaltyRuleVars};
use super::types::*;
use crate::controller::milp_interactions::{
    build_interactions, dispatched_net_power_expr, forecast_net_power_expr, AssetInteraction,
    GlobalMilpInputs, GridMilpVars, InteractionVars, MilpVarPool,
};
use crate::controller::milp_planner::{
    AssetMilpContext, CostWeights, DeclareMode, DeclaredAsset, ObjectivePart, Participation,
};

/// How a solve declares its decisions. The one place the three solves differ in what they build.
pub(super) enum Declaration<'a> {
    /// Phase 1: every decision free (the grid direction a binary), and no asset declares the
    /// auxiliaries only friction prices.
    Phase1,
    /// Phase 2: every decision free, and each asset declares its friction auxiliaries, priced
    /// by its own friction costs, because phase 2 minimises that friction.
    Phase2,
    /// The marginal-cost pass: every mode decision (grid direction, and each asset's own) is
    /// fixed to the winning solution's value as a *continuous* variable, because HiGHS returns
    /// no duals for a model with any integer column.
    Pinned(&'a SolveOutput),
}

impl Declaration<'_> {
    /// One slot's grid import/export exclusion variable.
    fn declare_u_grid(&self, t: usize, vars: &mut ProblemVariables) -> Variable {
        match self {
            Declaration::Phase1 | Declaration::Phase2 => vars.add(variable().binary()),
            Declaration::Pinned(winning) => {
                // u_grid is a mode decision like the asset binaries: fixed continuous, not
                // `.binary()`, for the reason given on `Declaration::Pinned`.
                let v = pinned_binary(Some(if winning.p_imp_kw[t] > 1e-6 { 1.0 } else { 0.0 }));
                vars.add(variable().min(v).max(v))
            }
        }
    }

    fn declare_asset<'c>(
        &self,
        ctx: &'c dyn AssetMilpContext,
        n: usize,
        vars: &mut ProblemVariables,
    ) -> Box<dyn DeclaredAsset + 'c> {
        let mode = match self {
            Declaration::Phase1 => DeclareMode::Plan { friction: false },
            Declaration::Phase2 => DeclareMode::Plan { friction: true },
            // R-98: through the same function the plan used, with the asset's own winning
            // decisions pinned, so the priced model is the planned one.
            Declaration::Pinned(winning) => DeclareMode::Pinned(
                winning
                    .asset(ctx.asset_id())
                    .expect("the winning solution holds every asset it was solved with"),
            ),
        };
        ctx.declare(n, mode, vars)
    }
}

struct ActiveInteraction {
    interaction: Box<dyn AssetInteraction>,
    vars: InteractionVars,
}

/// Everything declared before a solve adds its own objective: handles for every variable, the
/// grid-level inputs, the penalty slacks and the interactions that apply to this set of assets.
pub(super) struct ModelSkeleton<'c> {
    pub(super) global: GlobalMilpInputs,
    pub(super) pool: MilpVarPool<'c>,
    pub(super) penalty_vars: Vec<PenaltyRuleVars>,
    interactions: Vec<ActiveInteraction>,
}

impl<'c> ModelSkeleton<'c> {
    /// Declare the skeleton's variables, in this order: grid variables, the forecast
    /// participants' (PV's used generation, where the grid's own PV variable used to be), penalty
    /// slacks, the dispatched assets' variables, then the variables of every interaction that
    /// applies. `asset_contexts` lists the forecast participants first (`run_planner` sorts them
    /// so), and the pool keeps that order. Returns `ProblemVariables` apart from the handles so
    /// the caller can `minimise` it.
    pub(super) fn declare(
        inputs: &MilpInputs,
        p1w: &Phase1Weights,
        asset_contexts: &'c [Box<dyn AssetMilpContext>],
        declaration: Declaration<'_>,
    ) -> (ProblemVariables, Self) {
        let n = inputs.n;

        let global = GlobalMilpInputs {
            n,
            dt_h: inputs.dt_h.clone(),
            forecast_surplus_kw: inputs.forecast_surplus_kw(),
        };

        let mut vars = variables!();

        let p_imp: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();
        let p_exp: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();
        let u_grid: Vec<Variable> = (0..n)
            .map(|t| declaration.declare_u_grid(t, &mut vars))
            .collect();
        let s_imp_viol: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();
        let s_exp_viol: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();

        let mut pool = MilpVarPool {
            grid: GridMilpVars {
                p_imp,
                p_exp,
                u_grid,
                s_imp_viol,
                s_exp_viol,
            },
            assets: Vec::with_capacity(asset_contexts.len()),
        };

        let forecast_count = asset_contexts
            .iter()
            .take_while(|c| !matches!(c.participation(), Participation::Dispatched))
            .count();
        debug_assert!(
            asset_contexts[forecast_count..]
                .iter()
                .all(|c| matches!(c.participation(), Participation::Dispatched)),
            "the forecast participants come first among the asset contexts"
        );
        for ctx in &asset_contexts[..forecast_count] {
            pool.assets
                .push(declaration.declare_asset(ctx.as_ref(), n, &mut vars));
        }

        // WP6.3 (BL-09): one slack per window per active penalty rule, a free continuous slack
        // rather than a mode decision, so it is never pinned. Empty `penalty_rules` declares
        // nothing.
        let penalty_vars =
            penalty::declare_penalty_vars(&inputs.penalty_rules, &inputs.cum_s, &mut vars);

        for ctx in &asset_contexts[forecast_count..] {
            let declared = declaration.declare_asset(ctx.as_ref(), n, &mut vars);
            pool.assets.push(declared);
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
    /// each asset's own cost part (PV's full-use tie-break among them) and the interactions.
    ///
    /// This single expression is phase 1's objective, phase 2's cost cap and the marginal-cost
    /// pass's objective. They must be one expression: `c_star` (phase 1's optimum) has to equal the
    /// cap term for term, or phase 2 becomes infeasible outright, and the shadow price has to
    /// reflect the objective that was actually optimised.
    pub(super) fn cost_expr(&self, inputs: &MilpInputs, p1w: &Phase1Weights) -> Expression {
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
        let weights = CostWeights {
            c_storage_wear_eur_kwh: p1w.c_storage_wear_eur_kwh,
        };
        let cost = |asset: &&Box<dyn DeclaredAsset + 'c>| {
            asset.objective(ObjectivePart::Cost(&weights), n, &inputs.dt_h)
        };
        for asset in self.pool.assets.iter().filter(|a| a.dispatched()) {
            objective += cost(&asset);
        }
        for active in &self.interactions {
            objective += active.interaction.objective(&active.vars, &inputs.dt_h);
        }
        // The forecast participants' parts (PV's full-use tie-break) last: the order HiGHS
        // receives the terms in, and so the evaluated objective, stays what it was when the
        // tie-break was a grid term.
        for asset in self.pool.assets.iter().filter(|a| !a.dispatched()) {
            objective += cost(&asset);
        }
        objective
    }

    /// Phase 2's friction: each asset's friction part. No economic terms; those are capped by
    /// `cost_expr` instead. PV's full-use tie-break is in its friction part too, or the epsilon
    /// cost budget could let friction smoothing curtail PV arbitrarily.
    pub(super) fn friction_expr(&self, inputs: &MilpInputs) -> Expression {
        let mut friction = Expression::from(0.0);
        // Dispatched assets first, forecast participants last (see `cost_expr`).
        let (dispatched, forecast): (Vec<_>, Vec<_>) =
            self.pool.assets.iter().partition(|a| a.dispatched());
        for asset in dispatched.into_iter().chain(forecast) {
            friction += asset.objective(ObjectivePart::Friction, inputs.n, &inputs.dt_h);
        }
        friction
    }

    /// Add the power balance and grid bounds per slot, then every asset's, interaction's and
    /// penalty rule's constraints. Returns the model and the per-slot power-balance references,
    /// which the marginal-cost pass reads shadow prices off (the solve phases discard them).
    pub(super) fn add_constraints<S: SolverModel>(
        &self,
        mut model: S,
        inputs: &MilpInputs,
    ) -> (S, Vec<good_lp::constraint::ConstraintReference>) {
        let n = inputs.n;
        let pool = &self.pool;
        let grid = &pool.grid;
        let mut power_balance_refs = Vec::with_capacity(n);
        for t in 0..n {
            // Import covers the forecast participants' net power (PV's generation, the base
            // load), the dispatched assets', export and the user's baseline override. Two sums in
            // this order, so HiGHS receives the row as it did when PV and base load were grid
            // terms.
            let power_balance = constraint!(
                grid.p_imp[t]
                    - forecast_net_power_expr(pool, t)
                    - dispatched_net_power_expr(pool, t)
                    - grid.p_exp[t]
                    == inputs.load_adjust_kw[t]
            );
            probe!(constraint, &power_balance);
            power_balance_refs.push(model.add_constraint(power_balance));
            for asset in &pool.assets {
                for c in asset.slot_constraints(t) {
                    model = with_constraint(model, c);
                }
            }
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

        for asset in &pool.assets {
            for c in asset.constraints(n, &self.global.dt_h) {
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

    /// Extract a `SolveOutput` from a solved `good_lp::Solution`: the grid series here, each
    /// asset's part by the asset itself.
    pub(super) fn read_output<S: Solution>(
        &self,
        solution: &S,
        objective: &Expression,
        inputs: &MilpInputs,
    ) -> SolveOutput {
        let n = inputs.n;
        let grid = &self.pool.grid;
        let series =
            |vs: &[Variable]| -> Vec<f64> { (0..n).map(|t| solution.value(vs[t])).collect() };
        SolveOutput {
            status: solution.status(),
            objective_eur: solution.eval(objective),
            p_imp_kw: series(&grid.p_imp),
            p_exp_kw: series(&grid.p_exp),
            s_imp_viol_kw: series(&grid.s_imp_viol),
            s_exp_viol_kw: series(&grid.s_exp_viol),
            s_penalty_kw: penalty::read_penalty_solution(solution, &self.penalty_vars),
            assets: self
                .pool
                .assets
                .iter()
                .map(|asset| asset.read(solution, n))
                .collect(),
        }
    }

    /// Phase 2's warm start from phase 1's solution: the grid's own values, then each asset's,
    /// read from its own phase-1 solution.
    pub(super) fn warm_start(&self, p1: &SolveOutput) -> Vec<(Variable, f64)> {
        let n = self.global.n;
        let grid = &self.pool.grid;
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
        }
        for asset in &self.pool.assets {
            if let Some(own) = p1.asset(asset.asset_id()) {
                iv.extend(asset.warm_start(own, n));
            }
        }
        iv
    }
}

/// `model.with(c)`, recording `c` first when a test is capturing the model (`probe!`).
pub(super) fn with_constraint<S: SolverModel>(model: S, c: Constraint) -> S {
    probe!(constraint, &c);
    model.with(c)
}
