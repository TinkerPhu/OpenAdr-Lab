// ── What the EV's context knows about itself ─────────────────────────────────
// Read-only accessors over `EvMilpContext`: the floor a trip may not breach, the
// per-slot exogenous drop, the energy expression the comfort bands are paid out of,
// and the firm energy those bands must be able to cover. Split out of `ev_milp.rs`
// to stay under the VEN/src/ 500-production-line cap (`ven-architecture`), the same
// cross-file inherent-impl pattern `ev_session_context.rs` already uses.

use good_lp::Expression;

use crate::controller::milp_planner::asset_port::{EvMilpContext, EvMilpVars};

impl EvMilpContext {
    /// The SoC floor a predicted trip may not push the projection below — the
    /// same floor the live tick's `apply_return_drop` applies. 0.0 when no usage
    /// schedule declared one.
    ///
    /// Capped at the live reading: a car already sitting below its floor (a 2 %
    /// pack against a 5 % floor) must not make the site solve infeasible, and the
    /// floor's job is to stop a trip draining the pack, never to invent charge the
    /// vehicle does not have. In that degenerate case the floor is simply where
    /// the car already is.
    pub fn floor_frac(&self) -> f64 {
        self.soc_drops
            .as_ref()
            .map_or(0.0, |d| d.floor_frac)
            .min(self.soc_init)
            .max(0.0)
    }

    /// Slot `t`'s exogenous SoC drop as a fraction (0.0 in every slot where no
    /// predicted trip ends). One reader of `ExogenousSocDrops`, so the balance
    /// constraint and the variable bounds cannot disagree about it.
    pub fn drop_frac_at(&self, t: usize) -> f64 {
        self.soc_drops
            .as_ref()
            .and_then(|d| d.drop_frac_per_slot.get(t).copied())
            .unwrap_or(0.0)
            .max(0.0)
    }

    /// Total energy charged across the whole horizon [kWh].
    ///
    /// Whole-horizon, not deadline-bounded: with the obligation now a bound on
    /// `soc_ev[deadline_step]`, this expression's only job is to tie the priced
    /// bands to actual power so a reward cannot be banked without moving `p_ev`.
    /// Several obligations have several deadlines, so there is no single one to
    /// bound it by.
    pub fn energy_expr(&self, v: &EvMilpVars, n: usize, dt_h: &[f64]) -> Expression {
        let last = self.band_accounting_last_step(n);
        let mut expr = Expression::from(0.0);
        for (t, &dt) in dt_h.iter().enumerate().take(n) {
            if t <= last {
                expr += dt * v.p_ev[t];
            }
        }
        expr
    }

    /// Last step whose charging may be paid for out of the comfort bands.
    ///
    /// The latest obligation's deadline, or the whole horizon when there is no
    /// obligation at all (plain opportunistic charging, which has no deadline to be
    /// bounded by).
    ///
    /// Why this bound exists: `ev_comfort::ev_energy_segments` prices bands from the
    /// current state of charge all the way to a *full* pack, the part above
    /// `soc_target` at `v_ev_extra_eur_kwh`. R-93 briefly made this sum whole-horizon,
    /// which let the solver buy that beyond-target energy across all 48 h instead of
    /// only before the deadline. On the fleet that charged a VEN to 100 % against an
    /// 80 % target and refilled the predicted trip's drop in the very slot it
    /// occurred, so the planned state-of-charge curve showed no dip at all.
    fn band_accounting_last_step(&self, n: usize) -> usize {
        self.obligations
            .iter()
            .map(|o| o.deadline_step)
            .max()
            .unwrap_or_else(|| n.saturating_sub(1))
            .min(n.saturating_sub(1))
    }

    /// The firm energy the bands must be able to cover [kWh]. Only used to size the
    /// unpriced guarantee band — a promise is not conditional on a bid.
    ///
    /// An obligation's demand is measured from the live state of charge *minus what
    /// the trips before its deadline will consume*, not from the live reading alone.
    /// Measuring from `soc_init` only is right for one session and wrong the moment a
    /// trip sits between two: a car starting at 0.80, asked for 0.80 again after a
    /// 30 % trip, demanded "nothing" — so the band was zero-width, `ev_energy ==
    /// bought` forced every charging variable to zero, and the whole requirement was
    /// absorbed by the shortfall slack instead. The plan charged not at all, which is
    /// precisely the failure a stated trip distance exists to prevent.
    pub fn firm_required_kwh(&self) -> f64 {
        self.obligations
            .iter()
            .map(|o| {
                let consumed: f64 = (0..o.deadline_step.saturating_add(1))
                    .map(|t| self.drop_frac_at(t))
                    .sum();
                ((o.target_soc - self.soc_init + consumed) * self.battery_kwh).max(0.0)
            })
            .fold(0.0_f64, f64::max)
    }
}
