//! A forecast participant as the planner sees it (W3): a PV array or a base load, taking part in
//! the site balance from its own forecast without being dispatched. A curtailable one (PV)
//! declares how much of its forecast the plan uses, bounded by the forecast and nudged towards
//! using all of it; a non-curtailable one (base load) contributes exactly its forecast. It never
//! reads another asset's variables, and the plan never allocates it (`Participation::Forecast`).

use good_lp::{constraint, variable, Constraint, Expression, ProblemVariables, Solution, Variable};

use crate::controller::milp_planner::{
    AllocationBasis, AssetMilpContext, AssetSolution, AssetSolutionDetail, DeclareMode,
    DeclaredAsset, ForecastMilpContext, ForecastRole, ObjectivePart, Participation,
};

/// Tiny per-kWh bias favouring full use of curtailable generation [€/kWh]. Even though every
/// real cost term already favours using PV over curtailing it for free (see
/// `pv-export-curtailment` design), HiGHS's `with_mip_gap` tolerance on `u_grid` (grid
/// mutual-exclusion binary) can still accept a solution that curtails PV by a small,
/// cost-irrelevant amount rather than searching for the true optimum — found via
/// `pv_used_equals_forecast_when_no_export_constraint_binds` (no export cap at all, yet the
/// unbiased solver still curtailed). Small enough that any real constraint (export cap,
/// soft-violation penalty) still dominates and forces genuine curtailment when needed. Part of
/// both the cost and the friction objective, so phase 2's epsilon budget cannot trade it away.
pub const PV_USE_TIEBREAK_EUR_PER_KWH: f64 = 0.005;

/// One forecast participant's variables in one solve: the generation used per slot when it is
/// curtailable, none otherwise.
struct DeclaredForecast<'a> {
    ctx: &'a ForecastMilpContext,
    used_kw: Vec<Variable>,
}

impl ForecastMilpContext {
    /// Net power per unit of the asset's own power: generation feeds the site (negative),
    /// load draws from it (positive).
    fn sign(&self) -> f64 {
        match self.role {
            ForecastRole::Generation => -1.0,
            ForecastRole::Load => 1.0,
        }
    }
}

impl DeclaredAsset for DeclaredForecast<'_> {
    fn asset_id(&self) -> &str {
        &self.ctx.asset_id
    }

    fn net_power_expr(&self, t: usize) -> Expression {
        match self.used_kw.get(t) {
            Some(&used) => self.ctx.sign() * used,
            None => Expression::from(self.ctx.sign() * self.ctx.forecast_kw[t]),
        }
    }

    /// Every row is a slot's own (`slot_constraints`).
    fn constraints(&self, _n: usize, _dt_h: &[f64]) -> Vec<Constraint> {
        Vec::new()
    }

    /// A curtailable participant uses at most its forecast in the slot.
    fn slot_constraints(&self, t: usize) -> Vec<Constraint> {
        self.used_kw
            .get(t)
            .map(|&used| constraint!(used <= self.ctx.forecast_kw[t]))
            .into_iter()
            .collect()
    }

    /// No economic term of its own; curtailable generation carries the full-use tie-break in
    /// both parts.
    fn objective(&self, _part: ObjectivePart<'_>, _n: usize, dt_h: &[f64]) -> Expression {
        let mut expr = Expression::from(0.0);
        if self.ctx.role == ForecastRole::Generation {
            for (t, &used) in self.used_kw.iter().enumerate() {
                expr += -(PV_USE_TIEBREAK_EUR_PER_KWH * dt_h[t]) * used;
            }
        }
        expr
    }

    /// Phase 2 starts from the generation phase 1 used.
    fn warm_start(&self, own: &AssetSolution, _n: usize) -> Vec<(Variable, f64)> {
        self.used_kw
            .iter()
            .zip(&own.net_power_kw)
            .map(|(&used, &net_kw)| (used, (net_kw * self.ctx.sign()).max(0.0)))
            .collect()
    }

    fn read(&self, solution: &dyn Solution, n: usize) -> AssetSolution {
        let net_power_kw = (0..n)
            .map(|t| match self.used_kw.get(t) {
                Some(&used) => self.ctx.sign() * solution.value(used),
                None => self.ctx.sign() * self.ctx.forecast_kw[t],
            })
            .collect();
        AssetSolution {
            asset_id: self.ctx.asset_id.clone(),
            net_power_kw,
            planned_state: Vec::new(),
            storage: None,
            warnings: Vec::new(),
            detail: AssetSolutionDetail::Forecast,
        }
    }

    fn dispatched(&self) -> bool {
        false
    }
}

impl AssetMilpContext for ForecastMilpContext {
    fn solution_from_trace(
        &self,
        net_kw: &[f64],
        _states: &[std::collections::HashMap<String, f64>],
    ) -> AssetSolution {
        AssetSolution {
            asset_id: self.asset_id.clone(),
            net_power_kw: net_kw.to_vec(),
            planned_state: Vec::new(),
            storage: None,
            warnings: Vec::new(),
            detail: AssetSolutionDetail::Forecast,
        }
    }

    fn asset_id(&self) -> &str {
        &self.asset_id
    }

    /// The same declaration in every mode: the used generation is continuous, so there is no
    /// mode decision to pin.
    fn declare<'a>(
        &'a self,
        n: usize,
        _mode: DeclareMode<'_>,
        vars: &mut ProblemVariables,
    ) -> Box<dyn DeclaredAsset + 'a> {
        let used_kw = if self.curtailable {
            (0..n).map(|_| vars.add(variable().min(0.0))).collect()
        } else {
            Vec::new()
        };
        Box::new(DeclaredForecast { ctx: self, used_kw })
    }

    /// Never claims surplus: the plan does not allocate a forecast participant.
    fn surplus_claim_rank(&self) -> u8 {
        u8::MAX
    }

    /// Never priced as an allocation; see `participation`.
    fn allocation_basis(&self) -> AllocationBasis {
        AllocationBasis::Load
    }

    fn participation(&self) -> Participation<'_> {
        Participation::Forecast {
            role: self.role,
            forecast_kw: &self.forecast_kw,
        }
    }

    #[cfg(test)]
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use good_lp::IntoAffineExpression;

    /// A forecast participant's readback is the traced net power, signed as the site sees it.
    #[test]
    fn solution_from_trace_keeps_the_traced_net_power() {
        let sol = pv(vec![3.0, 4.0]).solution_from_trace(&[-3.0, -4.0], &[]);
        assert_eq!(sol.net_power_kw, vec![-3.0, -4.0]);
        assert!(matches!(sol.detail, AssetSolutionDetail::Forecast));
    }

    fn pv(forecast_kw: Vec<f64>) -> ForecastMilpContext {
        ForecastMilpContext {
            asset_id: "pv".into(),
            role: ForecastRole::Generation,
            forecast_kw,
            curtailable: true,
        }
    }

    fn base_load(forecast_kw: Vec<f64>) -> ForecastMilpContext {
        ForecastMilpContext {
            asset_id: "base_load".into(),
            role: ForecastRole::Load,
            forecast_kw,
            curtailable: false,
        }
    }

    /// PV feeds the site: its term in the balance is minus the generation it uses, one variable
    /// per slot, bounded by the forecast.
    #[test]
    fn a_curtailable_generator_declares_its_used_generation() {
        let ctx = pv(vec![3.0, 4.0]);
        let mut vars = good_lp::variables!();
        let declared = ctx.declare(2, DeclareMode::Plan { friction: false }, &mut vars);
        let coefficients: Vec<_> = declared.net_power_expr(1).linear_coefficients().collect();
        assert_eq!(coefficients.len(), 1);
        assert_eq!(coefficients[0].1, -1.0);
        assert!(declared.constraints(2, &[1.0, 1.0]).is_empty());
        assert_eq!(declared.slot_constraints(1).len(), 1);
        assert!(!declared.dispatched());
    }

    /// The base load draws exactly its forecast: a constant, no variable, no constraint.
    #[test]
    fn a_fixed_load_contributes_its_forecast() {
        let ctx = base_load(vec![0.5, 0.75]);
        let mut vars = good_lp::variables!();
        let declared = ctx.declare(2, DeclareMode::Plan { friction: false }, &mut vars);
        let e = declared.net_power_expr(1);
        assert_eq!(e.clone().linear_coefficients().count(), 0);
        assert_eq!(e.constant(), 0.75);
        assert!(declared.constraints(2, &[1.0, 1.0]).is_empty());
        assert!(declared.slot_constraints(1).is_empty());
    }

    /// Neither is dispatched: the plan follows them, it does not schedule them.
    #[test]
    fn forecast_participants_are_not_dispatched() {
        for ctx in [pv(vec![1.0]), base_load(vec![1.0])] {
            assert!(matches!(
                ctx.participation(),
                Participation::Forecast { forecast_kw, .. } if forecast_kw == [1.0]
            ));
        }
    }

    /// Curtailable generation is nudged towards full use in both objective parts; a load has no
    /// objective term.
    #[test]
    fn only_curtailable_generation_carries_the_full_use_tiebreak() {
        let mut vars = good_lp::variables!();
        let gen = pv(vec![2.0]);
        let declared = gen.declare(1, DeclareMode::Plan { friction: true }, &mut vars);
        for part in [
            ObjectivePart::Friction,
            ObjectivePart::Cost(&crate::controller::milp_planner::CostWeights {
                c_storage_wear_eur_kwh: 0.0,
            }),
        ] {
            let coefficients: Vec<_> = declared
                .objective(part, 1, &[0.5])
                .linear_coefficients()
                .map(|(_, c)| c)
                .collect();
            assert_eq!(coefficients, vec![-(PV_USE_TIEBREAK_EUR_PER_KWH * 0.5)]);
        }
        let load = base_load(vec![1.0]);
        let declared = load.declare(1, DeclareMode::Plan { friction: true }, &mut vars);
        assert_eq!(
            declared
                .objective(ObjectivePart::Friction, 1, &[0.5])
                .linear_coefficients()
                .count(),
            0
        );
    }
}
