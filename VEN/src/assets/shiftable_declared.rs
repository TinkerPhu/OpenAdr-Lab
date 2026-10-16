//! A shiftable load as the planner sees it: one start choice per valid start slot, exactly one
//! of which is taken, its fixed power in every slot the run covers, the earliest-start
//! tie-break, and the window it declares for the plan's session flexibility. The window itself
//! is resolved when the context is built (`shiftable_load.rs`). It never reads another asset's
//! variables.

use chrono::{DateTime, Utc};
use good_lp::{constraint, variable, Constraint, Expression, ProblemVariables, Solution, Variable};

use crate::controller::milp_planner::asset_port::pinned_binary;
use crate::controller::milp_planner::{
    AllocationBasis, AssetMilpContext, AssetSolution, AssetSolutionDetail, DeclareMode,
    DeclaredAsset, FlexibilityWindow, ObjectivePart, ShiftableLoadMilpContext,
};

/// Where this kind stands in claiming PV surplus, lower first: in the plan's allocation
/// order (`AssetMilpContext::surplus_claim_rank`) and in live opportunistic dispatch
/// (`Asset::surplus_claim_rank`), one declaration for both. A shiftable load claims PV surplus after the EV and the heater.
pub(crate) const SURPLUS_CLAIM_RANK: u8 = 2;

/// Deterministic earliest-start tie-break for a shiftable load [€ per start-slot index].
///
/// Window offsets are measured from the ALIGNED grid start (now truncated to the
/// Zone-A slot boundary), so a request posted mid-slot often has several
/// cost-equal valid start slots under flat tariffs. Without a bias HiGHS's
/// arbitrary pick can schedule the load a full slot later than necessary
/// (found via the isolated E2E tail diagnostic, 2026-07-12). Small enough that
/// any real per-slot cost difference dominates (2 kW × 0.5 h × 0.01 €/kWh =
/// 0.01 € ≫ 0.001 €); part of both the cost and the friction objective, so phase 2's
/// epsilon budget cannot trade the early start away.
pub const SHIFT_TIEBREAK_EUR_PER_SLOT: f64 = 0.001;

/// One shiftable load's start choices in one solve.
struct DeclaredShiftableLoad<'a> {
    ctx: &'a ShiftableLoadMilpContext,
    /// One start choice per entry in `ctx.valid_start_slots`.
    y_shift: Vec<Variable>,
}

impl ShiftableLoadMilpContext {
    /// The one shiftable-load variable declaration (R-98): one start choice per valid start
    /// slot, free for the plan, fixed to the winning *start* for the marginal-cost pass.
    /// (The pass once re-derived starts from the slots the load ran in, which pins two starts
    /// for a two-slot run and makes "exactly one start" infeasible.)
    fn declare_starts(&self, vars: &mut ProblemVariables, pinned: Option<&[f64]>) -> Vec<Variable> {
        (0..self.valid_start_slots.len())
            .map(|i| match pinned {
                None => vars.add(variable().binary()),
                Some(starts) => {
                    let v = pinned_binary(starts.get(i).copied());
                    vars.add(variable().min(v).max(v))
                }
            })
            .collect()
    }

    /// The start choices whose run covers slot `t`, as (choice index, start slot).
    fn starts_covering(&self, t: usize) -> impl Iterator<Item = usize> + '_ {
        self.valid_start_slots
            .iter()
            .enumerate()
            .filter(move |(_, &j)| t >= j && t < j + self.duration_slots)
            .map(|(ji, _)| ji)
    }
}

/// The load's own winning start choices, which only a shiftable-load solution carries.
fn own_starts(own: &AssetSolution) -> &[f64] {
    match &own.detail {
        AssetSolutionDetail::ShiftableLoad { starts } => starts,
        other => unreachable!("a shiftable load is pinned to its own solution, got {other:?}"),
    }
}

impl DeclaredAsset for DeclaredShiftableLoad<'_> {
    fn asset_id(&self) -> &str {
        &self.ctx.asset_id
    }

    fn net_power_expr(&self, t: usize) -> Expression {
        let mut e = Expression::from(0.0);
        for ji in self.ctx.starts_covering(t) {
            e += self.ctx.power_kw * self.y_shift[ji];
        }
        e
    }

    /// Exactly one start slot is chosen: the hard-window requirement (spec.md "Planner never
    /// schedules a start outside the valid window").
    fn constraints(&self, _n: usize, _dt_h: &[f64]) -> Vec<Constraint> {
        let mut sum_y = Expression::from(0.0);
        for &y in &self.y_shift {
            sum_y += y;
        }
        vec![constraint!(sum_y == 1.0)]
    }

    /// No economic term of its own; the earliest-start tie-break is in both parts.
    fn objective(&self, _part: ObjectivePart<'_>, _n: usize, _dt_h: &[f64]) -> Expression {
        let mut expr = Expression::from(0.0);
        for (ji, &y) in self.y_shift.iter().enumerate() {
            expr += SHIFT_TIEBREAK_EUR_PER_SLOT * (self.ctx.valid_start_slots[ji] as f64) * y;
        }
        expr
    }

    /// Phase 2 finds a start from scratch; the load has no warm start.
    fn warm_start(&self, _own: &AssetSolution, _n: usize) -> Vec<(Variable, f64)> {
        Vec::new()
    }

    fn read(&self, solution: &dyn Solution, n: usize) -> AssetSolution {
        let mut net_power_kw = vec![0.0; n];
        for (t, slot_kw) in net_power_kw.iter_mut().enumerate() {
            for ji in self.ctx.starts_covering(t) {
                *slot_kw += self.ctx.power_kw * solution.value(self.y_shift[ji]);
            }
        }
        AssetSolution {
            asset_id: self.ctx.asset_id.clone(),
            net_power_kw,
            planned_state: Vec::new(),
            storage: None,
            warnings: Vec::new(),
            detail: AssetSolutionDetail::ShiftableLoad {
                starts: self.y_shift.iter().map(|&y| solution.value(y)).collect(),
            },
        }
    }
}

impl AssetMilpContext for ShiftableLoadMilpContext {
    fn asset_id(&self) -> &str {
        &self.asset_id
    }

    fn declare<'a>(
        &'a self,
        _n: usize,
        mode: DeclareMode<'_>,
        vars: &mut ProblemVariables,
    ) -> Box<dyn DeclaredAsset + 'a> {
        let pinned = match mode {
            DeclareMode::Plan { .. } => None,
            DeclareMode::Pinned(own) => Some(own_starts(own)),
        };
        Box::new(DeclaredShiftableLoad {
            ctx: self,
            y_shift: self.declare_starts(vars, pinned),
        })
    }

    /// A shiftable load claims PV surplus after the EV and the heater.
    fn surplus_claim_rank(&self) -> u8 {
        SURPLUS_CLAIM_RANK
    }

    #[cfg(test)]
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn allocation_basis(&self) -> AllocationBasis {
        AllocationBasis::Load
    }

    /// The load's own window, at its one fixed power, for the energy of one full run.
    fn flexibility_window(&self, _now: DateTime<Utc>) -> Option<FlexibilityWindow> {
        let energy_needed_kwh =
            crate::entities::units::energy_kwh_from_min(self.power_kw, self.duration_min as f64);
        (energy_needed_kwh > 0.0).then_some(FlexibilityWindow {
            energy_needed_kwh,
            power_min_kw: self.power_kw,
            power_max_kw: self.power_kw,
            window_start: self.earliest_start,
            window_end: self.latest_end,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::test_support::milp_pool::bound_range;

    fn washing_machine() -> ShiftableLoadMilpContext {
        let now = chrono::Utc::now();
        ShiftableLoadMilpContext {
            asset_id: "wm".into(),
            power_kw: 2.0,
            duration_slots: 2,
            valid_start_slots: vec![0, 1, 2],
            duration_min: 60,
            earliest_start: now,
            latest_end: now + chrono::Duration::hours(2),
        }
    }

    /// R-98: the marginal-cost pass declares a shiftable load through the plan's own
    /// declaration, with exactly the winning *start* pinned — not every slot it runs in.
    #[test]
    fn declare_pinned_vars_pins_the_winning_start_only() {
        let ctx = washing_machine();
        let winning = vec![0.0, 1.0, 0.0];
        let declared = |pinned: bool| {
            let mut vars = good_lp::variables!();
            ctx.declare_starts(&mut vars, pinned.then_some(&winning[..]))
                .len()
        };
        assert_eq!(declared(true), declared(false));

        let range = bound_range(|vars| ctx.declare_starts(vars, Some(&winning)));
        assert_eq!(range, vec![(0.0, 0.0), (1.0, 1.0), (0.0, 0.0)]);
    }

    /// The load adds its power to every slot its run covers, under each start that covers it.
    #[test]
    fn net_power_counts_each_start_covering_the_slot() {
        let ctx = washing_machine();
        let mut vars = good_lp::variables!();
        let declared = ctx.declare(4, DeclareMode::Plan { friction: false }, &mut vars);
        // A two-slot run: slot 0 only by the run starting at 0, slot 1 by the runs starting at
        // 0 and at 1.
        let covered = |t: usize| {
            let expr = declared.net_power_expr(t);
            good_lp::IntoAffineExpression::linear_coefficients(expr).count()
        };
        assert_eq!(covered(0), 1);
        assert_eq!(covered(1), 2);
    }
}
