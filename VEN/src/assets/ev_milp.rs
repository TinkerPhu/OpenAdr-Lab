// ── EV MILP plugin types ──────────────────────────────────────────────────────
// Struct/enum definitions live in `controller::milp_planner::asset_port`.
// Method implementations below are cross-file inherent impl blocks — valid Rust.

use good_lp::{constraint, variable, Constraint, Expression, ProblemVariables, Solution, Variable};

// Used only by the test module below, which refers to them through `super::`.
#[cfg(test)]
use {
    super::EvCharger,
    chrono::{DateTime, Utc},
};

use crate::controller::milp_planner::asset_port::{
    pinned_binary, EvMilpContext, EvMilpMode, EvMilpVars, EvSolOutput, ModeDecisions,
};

/// Penalty for letting the floor absorb part of a predicted trip's SoC drop
/// [EUR per SoC fraction]. The drop is a fact, not a choice: this only has to be
/// expensive enough that the solver never prefers dodging it to charging, while
/// staying finite so a drop deeper than the pack is floored rather than
/// infeasible (R-93 design Decision 3).
const DROP_UNMET_PENALTY_EUR: f64 = 1.0e5;

/// Penalty for missing a firm obligation [EUR per SoC fraction]. Above any
/// comfort bid or tariff spread, so shortfall is only ever bought when the window
/// physically cannot deliver — but finite, so an unreachable target degrades to a
/// reported gap instead of an infeasible site solve.
const SHORTFALL_PENALTY_EUR: f64 = 1.0e4;

/// Lateness cost every EV plan pays, ASAP or not [EUR per kWh per hour of delay].
/// When several slots cost the same, which one the EV charges in is a tie, and HiGHS
/// breaks ties arbitrarily: ven-1 (2026-10-05) got its battery-fed night charging in
/// 13 runs, 14 of them single 1.4 kW slots, which phase 2 had no time to merge. This
/// weight makes the tie "earliest first", and earliest first is contiguous. It is a
/// tie-breaker, not a preference: at most 0.005 EUR/kWh over a 48 h horizon, below
/// every tariff step. Measured with `bench_ven1_ev_fragmentation` and a fleet sweep:
/// 1e-5 leaves the ties unbroken, 1e-4 gives 4 runs and changes no site's import or
/// export.
const TIE_BREAK_LATENESS_EUR_KWH_H: f64 = 1.0e-4;

impl EvMilpContext {
    /// Declare all LP variables for this EV charger. Context-side canonical implementation.
    pub fn declare_vars(
        &self,
        n: usize,
        c_startup_eur: f64,
        c_ramp_eur_kw: f64,
        vars: &mut ProblemVariables,
    ) -> EvMilpVars {
        self.declare_vars_with(n, c_startup_eur, c_ramp_eur_kw, vars, ModeDecisions::Free)
    }

    /// The one EV variable declaration (R-98): the plan and the marginal-cost pass differ
    /// only in how `z_ev_on` is declared, so the SoC ceiling, the guarantee band and the
    /// `MustNotRun` shape exist once. A pinned pass carries no startup/ramp auxiliaries.
    pub fn declare_vars_with(
        &self,
        n: usize,
        c_startup_eur: f64,
        c_ramp_eur_kw: f64,
        vars: &mut ProblemVariables,
        modes: ModeDecisions,
    ) -> EvMilpVars {
        let (c_startup_eur, c_ramp_eur_kw) = match modes {
            ModeDecisions::Free => (c_startup_eur, c_ramp_eur_kw),
            ModeDecisions::Pinned(_) => (0.0, 0.0),
        };
        let p_ev = (0..n)
            .map(|_| {
                if self.mode == EvMilpMode::MustNotRun {
                    vars.add(variable().min(0.0).max(0.0))
                } else {
                    vars.add(variable().min(0.0).max(self.p_max_kw))
                }
            })
            .collect();
        // R-93: the SoC state itself, following `battery_milp.rs`'s `e_bat` —
        // index 0 pinned to the live reading by equal bounds, the rest free
        // between the floor and a full pack.
        let floor_frac = self.floor_frac();
        let soc_ev = (0..=n)
            .map(|i| {
                if i == 0 {
                    vars.add(variable().min(self.soc_init).max(self.soc_init))
                } else {
                    // The charge limit is the upper bound, not a full pack: above it
                    // the charger reports no import capability at all, so planning
                    // past it promises energy that will be refused. Held at or above
                    // `soc_init` so a vehicle already over its limit (a lowered limit,
                    // an injected state) stays feasible rather than unsolvable.
                    vars.add(variable().min(floor_frac).max(self.soc_ceiling()))
                }
            })
            .collect();
        // Bounded by its own slot's drop, so it can only ever absorb what the
        // floor would have cut — zero-width in every slot without a return.
        let drop_unmet = (0..n)
            .map(|t| vars.add(variable().min(0.0).max(self.drop_frac_at(t))))
            .collect();
        let shortfall_soc = self
            .obligations
            .iter()
            .map(|o| vars.add(variable().min(0.0).max(o.target_soc.max(0.0))))
            .collect();
        let z_ev_on = (0..n)
            .map(|t| {
                if self.mode == EvMilpMode::MustNotRun {
                    return vars.add(variable().min(0.0).max(0.0));
                }
                match modes {
                    ModeDecisions::Free => {
                        let ub = if self.a_ev[t] { 1.0 } else { 0.0 };
                        vars.add(variable().max(ub).binary())
                    }
                    ModeDecisions::Pinned(w) => {
                        let v = pinned_binary(w.z_ev_on.get(t).copied());
                        vars.add(variable().min(v).max(v))
                    }
                }
            })
            .collect();
        // `ev-comfort-piecewise-core`: one continuous variable per priced band.
        // Because the bids are non-increasing (enforced by
        // `entities::comfort::validate_curve`) the solver fills the valuable
        // bands first on its own — no ordering constraints and no binary.
        let e_seg: Vec<Variable> = if self.mode == EvMilpMode::MustNotRun {
            Vec::new()
        } else {
            let mut v: Vec<Variable> = self
                .segments
                .iter()
                .map(|seg| vars.add(variable().min(0.0).max(seg.kwh)))
                .collect();
            // A guarantee is not conditional on a bid: any requirement the bands
            // do not cover gets one more band at zero reward, delivered because
            // it was promised rather than because it is worth something.
            let priced_kwh: f64 = self.segments.iter().map(|s| s.kwh).sum();
            let uncovered = self.firm_required_kwh() - priced_kwh;
            if uncovered > 1e-9 {
                v.push(vars.add(variable().min(0.0).max(uncovered)));
            }
            v
        };
        let e_ev_extra = if self.mode == EvMilpMode::MustNotRun {
            vars.add(variable().min(0.0).max(0.0))
        } else {
            vars.add(variable().min(0.0).max(self.e_extra_max_kwh))
        };
        let delta_ev = if self.mode != EvMilpMode::MustNotRun && n > 1 && c_startup_eur > 0.0 {
            (0..n - 1).map(|_| vars.add(variable().binary())).collect()
        } else {
            vec![]
        };
        let delta_ev_ramp = if self.mode != EvMilpMode::MustNotRun && n > 1 && c_ramp_eur_kw > 0.0 {
            (0..n - 1).map(|_| vars.add(variable().min(0.0))).collect()
        } else {
            vec![]
        };
        EvMilpVars {
            p_ev,
            soc_ev,
            drop_unmet,
            shortfall_soc,
            z_ev_on,
            e_seg,
            e_ev_extra,
            delta_ev,
            delta_ev_ramp,
            p_min_kw: self.p_min_kw,
            battery_kwh: self.battery_kwh,
        }
    }

    /// Generate all MILP constraints for this EV charger. Context-side canonical implementation.
    /// `dt_h[t]` is the slot duration in hours for slot `t`.
    pub fn constraints(&self, v: &EvMilpVars, n: usize, dt_h: &[f64]) -> Vec<Constraint> {
        let mut cs: Vec<Constraint> = Vec::new();
        let ev_energy = self.energy_expr(v, n, dt_h);

        for t in 0..n {
            if self.mode != EvMilpMode::MustNotRun {
                let ev_ub = if self.a_ev[t] { self.p_max_kw } else { 0.0 };
                cs.push(constraint!(v.p_ev[t] >= self.p_min_kw * v.z_ev_on[t]));
                cs.push(constraint!(v.p_ev[t] <= ev_ub * v.z_ev_on[t]));
            }
        }
        // WP4.1 (BL-28): free-energy gating — charging may not exceed the
        // per-slot free cap (PV surplus / non-positive-tariff slots).
        if let Some(cap) = &self.p_free_cap_kw {
            for (t, &cap_kw) in cap.iter().enumerate().take(n) {
                cs.push(constraint!(v.p_ev[t] <= cap_kw));
            }
        }
        // WP4.1-c (BL-28) MAX_COST: total charging cost, priced at the import
        // rate per slot, may not exceed the session budget.
        if let (Some(budget_eur), Some(c_imp)) = (self.budget_eur, &self.c_imp_eur_kwh) {
            let mut cost = Expression::from(0.0);
            for (t, &dt) in dt_h.iter().enumerate().take(n.min(c_imp.len())) {
                cost += (c_imp[t] * dt) * v.p_ev[t];
            }
            cs.push(constraint!(cost <= budget_eur));
        }
        if self.mode != EvMilpMode::MustNotRun {
            // Energy charged is exactly what was bought: the priced bands, or the
            // capped `e_ev_extra` for the modes that price per slot. R-18: an
            // equality, so a reward cannot be "banked" without moving p_ev.
            let mut bought = Expression::from(0.0);
            for seg in &v.e_seg {
                bought += *seg;
            }
            bought += v.e_ev_extra;
            cs.push(constraint!(ev_energy == bought));
        }

        // ── R-93: the SoC state itself ───────────────────────────────────────
        // Charging and the exogenous trip drop chained across the horizon, so the
        // plan's SoC curve is solved rather than reconstructed afterwards. Holds
        // in every mode, `MustNotRun` included: an unplugged EV still has a SoC,
        // and a predicted trip still consumes it.
        for (t, &dt) in dt_h.iter().enumerate().take(n) {
            let drop_frac = self.drop_frac_at(t);
            // `drop_unmet` is bounded by this slot's own drop (see `declare_vars`)
            // and penalised, so it can only absorb the part of the drop the floor
            // would have cut — mirroring the live tick's `(soc - drop).max(floor)`
            // instead of making a deep trip infeasible.
            cs.push(constraint!(
                v.soc_ev[t + 1]
                    == v.soc_ev[t] + (dt / self.battery_kwh) * v.p_ev[t] - drop_frac
                        + v.drop_unmet[t]
            ));
        }

        // ── Each obligation binds the SoC at its own deadline ────────────────
        // A firm target is a guarantee, not a bid. The slack makes an unreachable
        // one a reported gap instead of an infeasible site solve — the behaviour
        // the old pre-solve `min(required, reachable)` clamp produced, now
        // expressed by the model so `ev_diagnostics` can report what was actually
        // missed, and for which session.
        for (k, ob) in self.obligations.iter().enumerate() {
            if self.mode == EvMilpMode::MustNotRun || ob.target_soc <= 1e-9 {
                continue;
            }
            // An obligation whose deadline falls outside this horizon constrains
            // nothing here; the next cycle will see it.
            if ob.deadline_step >= n {
                continue;
            }
            // `soc_ev[t]` is the state at the *start* of slot `t`, so energy
            // charged during the deadline slot itself lands at `deadline_step + 1`
            // — the same slots the previous cumulative-energy sum counted
            // (`0..=t_dead_step` inclusive). Binding at `deadline_step` instead
            // would quietly move every existing deadline one slot earlier.
            let step = ob.deadline_step + 1;
            cs.push(constraint!(
                v.soc_ev[step] + v.shortfall_soc[k] >= ob.target_soc
            ));
        }
        for i in 0..v.delta_ev.len() {
            let t = i + 1;
            cs.push(constraint!(
                v.delta_ev[i] >= v.z_ev_on[t] - v.z_ev_on[t - 1]
            ));
        }
        for i in 0..v.delta_ev_ramp.len() {
            let t = i + 1;
            cs.push(constraint!(v.delta_ev_ramp[i] >= v.p_ev[t] - v.p_ev[t - 1]));
            cs.push(constraint!(v.delta_ev_ramp[i] >= v.p_ev[t - 1] - v.p_ev[t]));
        }
        cs
    }

    /// EV objective contribution. Context-side canonical implementation.
    /// `dt_h[t]` is the slot duration in hours for slot `t` (ASAP lateness term).
    pub fn objective(
        &self,
        v: &EvMilpVars,
        startup_eur: f64,
        ramp_eur_kw: f64,
        w_services: f64,
        n: usize,
        dt_h: &[f64],
    ) -> Expression {
        let mut obj = Expression::from(0.0);
        for t in 1..n {
            if let Some(&d) = v.delta_ev.get(t - 1) {
                obj += startup_eur * d;
            }
            if let Some(&d) = v.delta_ev_ramp.get(t - 1) {
                obj += ramp_eur_kw * d;
            }
        }
        if self.mode != EvMilpMode::MustNotRun && !self.reward_per_slot {
            obj += -(w_services * self.v_extra_eur_kwh) * v.e_ev_extra;
            // BL-17 comfort bidding: CO2 analogue, already monetized via w_ghg.
            obj += -(w_services * self.v_extra_co2_eur_kwh) * v.e_ev_extra;
        }
        // Each band pays its own bid, so a partial charge earns partial value
        // (CO2 bid already monetized in). `zip` stops short, so the synthetic
        // guarantee band from `declare_vars` earns nothing — as it should.
        if self.mode != EvMilpMode::MustNotRun {
            for (seg, var) in self.segments.iter().zip(v.e_seg.iter()) {
                obj += -(w_services * seg.eur_per_kwh) * *var;
            }
        }
        // WP4.1 (BL-28) OPPORTUNISTIC / *_FREE / MAX_COST: reward the energy
        // actually charged, per slot, rather than the lump e_ev_extra reward
        // above — these modes want the reward biased toward specific slots
        // (free-energy gating, early-slot bias), which a lump sum can't express.
        // ASAP_FREE additionally biases the reward toward earlier slots
        // (up to +100 %, decaying to 0 across the horizon) so free energy is
        // taken as soon as it appears without ever making later slots
        // unprofitable. The bias must be steep enough that phase 2's friction
        // smoothing (cost cap phase2_epsilon_eur) cannot smear the allocation
        // back toward later slots.
        if self.reward_per_slot && self.mode != EvMilpMode::MustNotRun {
            let total_h: f64 = dt_h.iter().take(n).sum();
            let mut elapsed_h = 0.0;
            for (t, &dt) in dt_h.iter().enumerate().take(n) {
                let bias = if self.free_early_bias && total_h > 0.0 {
                    1.0 + (1.0 - elapsed_h / total_h)
                } else {
                    1.0
                };
                obj += -(w_services * self.v_extra_eur_kwh * bias * dt) * v.p_ev[t];
                elapsed_h += dt;
            }
        }
        // WP4.1 (BL-28) ASAP: every kWh pays €/kWh per hour of delay from now,
        // so the solver front-loads at maximum feasible rate, tariff-blind.
        // Outside ASAP the same term at `TIE_BREAK_LATENESS_EUR_KWH_H` decides
        // equal-cost slots: earliest first, which is one run instead of scattered
        // single slots.
        let lateness_eur_kwh_h = self
            .asap_lateness_eur_kwh_h
            .max(TIE_BREAK_LATENESS_EUR_KWH_H);
        if self.mode != EvMilpMode::MustNotRun {
            let mut elapsed_h = 0.0;
            for (t, &dt) in dt_h.iter().enumerate().take(n) {
                let mid_h = elapsed_h + dt / 2.0;
                obj += (lateness_eur_kwh_h * mid_h * dt) * v.p_ev[t];
                elapsed_h += dt;
            }
        }
        // R-93: both slacks exist so a physical fact (a deep trip) or an
        // impossible promise degrades gracefully instead of making the site solve
        // infeasible. Neither may ever be cheaper than charging, so both are
        // priced far above any tariff or comfort bid — and deliberately outside
        // `w_services`, since they express model integrity, not a preference.
        for u in v.drop_unmet.iter().take(n) {
            obj += DROP_UNMET_PENALTY_EUR * *u;
        }
        for sf in &v.shortfall_soc {
            obj += SHORTFALL_PENALTY_EUR * *sf;
        }
        obj
    }

    /// Read back the EV solution. Associated function (no `self` needed).
    ///
    /// The per-obligation SoC shortfall is converted back into the kWh the
    /// diagnostic reports, using the pack size cached on the vars.
    pub fn read_solution(sol: &impl Solution, v: &EvMilpVars, n: usize) -> EvSolOutput {
        EvSolOutput {
            p_ev_kw: (0..n).map(|t| sol.value(v.p_ev[t])).collect(),
            // R-93: the plan's SoC curve, straight off the solved variables —
            // there is no second integrator to disagree with it.
            soc_ev: (0..=n).map(|t| sol.value(v.soc_ev[t])).collect(),
            shortfall_kwh: v
                .shortfall_soc
                .iter()
                .map(|sf| sol.value(*sf) * v.battery_kwh)
                .collect(),
            z_ev_on: (0..n).map(|t| sol.value(v.z_ev_on[t])).collect(),
            e_ev_extra_kwh: sol.value(v.e_ev_extra),
            e_seg_kwh: v.e_seg.iter().map(|s| sol.value(*s)).sum(),
        }
    }
}

impl crate::controller::milp_planner::AssetMilpContext for EvMilpContext {
    fn asset_id(&self) -> &str {
        crate::ids::ASSET_EV
    }

    fn asset_kind(&self) -> crate::controller::milp_planner::AssetKind {
        crate::controller::milp_planner::AssetKind::Ev
    }

    fn milp_params(
        &self,
        _n: usize,
        _now: chrono::DateTime<chrono::Utc>,
    ) -> crate::controller::milp_planner::AssetMilpParams {
        use crate::controller::milp_planner::MilpLoadMode;
        let mode = match self.mode {
            EvMilpMode::MustRun => MilpLoadMode::MustRun,
            EvMilpMode::MayRun => MilpLoadMode::MayRun,
            EvMilpMode::MustNotRun => MilpLoadMode::MustNotRun,
        };
        crate::controller::milp_planner::AssetMilpParams::Ev(
            crate::controller::milp_planner::EvScalars {
                mode,
                soc_init: self.soc_init,
                a_ev: self.a_ev.clone(),
                soc_drops: self.soc_drops.clone(),
                obligations: self.obligations.clone(),
                p_max_kw: self.p_max_kw,
                p_min_kw: self.p_min_kw,
                battery_kwh: self.battery_kwh,
                segments: self.segments.clone(),
                e_extra_max_kwh: self.e_extra_max_kwh,
                v_extra_eur_kwh: self.v_extra_eur_kwh,
                budget_eur: self.budget_eur,
            },
        )
    }

    fn declare_vars_into_pool(
        &self,
        n: usize,
        c_startup_eur: f64,
        c_ramp_eur_kw: f64,
        vars: &mut ProblemVariables,
        pool: &mut crate::controller::milp_interactions::MilpVarPool,
    ) {
        pool.ev = Some(self.declare_vars(n, c_startup_eur, c_ramp_eur_kw, vars));
    }

    fn declare_pinned_vars_into_pool(
        &self,
        n: usize,
        winning: &crate::controller::milp_planner::asset_port::WinningModeDecisions,
        vars: &mut ProblemVariables,
        pool: &mut crate::controller::milp_interactions::MilpVarPool,
    ) {
        pool.ev = Some(self.declare_vars_with(n, 0.0, 0.0, vars, ModeDecisions::Pinned(winning)));
    }

    fn constraints(
        &self,
        pool: &crate::controller::milp_interactions::MilpVarPool,
        n: usize,
        dt_h: &[f64],
    ) -> Vec<Constraint> {
        // SAFETY: declare_vars_into_pool() (above) always runs before constraints()
        // for this context — see the AssetMilpContext trait's call-order invariant.
        EvMilpContext::constraints(self, pool.ev.as_ref().unwrap(), n, dt_h)
    }

    fn objective(
        &self,
        pool: &crate::controller::milp_interactions::MilpVarPool,
        n: usize,
        dt_h: &[f64],
        _c_wear_eur_kwh: f64,
        c_startup_eur: f64,
        c_ramp_eur_kw: f64,
    ) -> Expression {
        // Phase 1 (c_startup=0): no friction, service reward active (w_services=1.0).
        // Phase 2 friction (c_startup>0): startup+ramp active, service reward off.
        let (startup, ramp, w_services) = if c_startup_eur == 0.0 {
            (0.0_f64, 0.0_f64, 1.0_f64)
        } else {
            (c_startup_eur, c_ramp_eur_kw, 0.0_f64)
        };
        // SAFETY: declare_vars_into_pool() always runs before objective() for this
        // context — see the AssetMilpContext trait's call-order invariant.
        EvMilpContext::objective(
            self,
            pool.ev.as_ref().unwrap(),
            startup,
            ramp,
            w_services,
            n,
            dt_h,
        )
    }

    fn inject_grid_slots(&mut self, c_imp_eur_kwh: &[f64], p_pv_kw: &[f64], p_base_kw: &[f64]) {
        // WP4.1-c MAX_COST: keep the per-slot import rates for the budget constraint.
        if self.budget_eur.is_some() {
            self.c_imp_eur_kwh = Some(c_imp_eur_kwh.to_vec());
        }
        if !self.free_only {
            return;
        }
        // Free energy per slot: forecast PV surplus over the baseline load,
        // opened fully when the grid pays (or charges nothing) for import.
        let cap: Vec<f64> = c_imp_eur_kwh
            .iter()
            .zip(p_pv_kw)
            .zip(p_base_kw)
            .map(|((&c_imp, &pv), &base)| {
                if c_imp <= 0.0 {
                    self.p_max_kw
                } else {
                    (pv - base).max(0.0).min(self.p_max_kw)
                }
            })
            .collect();
        self.p_free_cap_kw = Some(cap);
    }
}

#[cfg(test)]
mod milp_context_trait_tests {
    use super::*;
    use crate::controller::milp_planner::{
        AssetKind, AssetMilpContext, AssetMilpParams, MilpLoadMode,
    };
    use crate::services::test_support::milp_pool::{bound_range, empty_pool};
    use good_lp::variables;

    // ── ev-usage-forecast: apply_usage_forecast ──────────────────────

    /// Build an `EvCharger` whose usage schedule is in the given mode, leaving
    /// 08:00 and returning 17:00 every day with no jitter.
    fn ev_with_usage(mode: crate::entities::asset_params::EvUsageMode) -> super::EvCharger {
        use crate::entities::asset_params::{EvUsageDayParams, EvUsageSimParams};
        use chrono::NaiveTime;
        let day = EvUsageDayParams {
            leave_time: NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
            leave_jitter_min: 0.0,
            return_time: NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
            return_jitter_min: 0.0,
            leave_probability: 1.0,
            soc_drop_pct_mean: 20.0,
            soc_drop_pct_stddev: 0.0,
        };
        super::EvCharger {
            max_charge_kw: 7.4,
            max_discharge_kw: 0.0,
            v2g_capable: false,
            battery_kwh: 60.0,
            consumption_kwh_per_km: 0.18,
            soc_target: 0.8,
            soc_target_profile: 0.8,
            default_charge_kw: 7.4,
            min_soc: 0.0,
            min_charge_kw: 0.0,
            response_delay_s: 0.0,
            departure_time: None,
            usage_sim: Some(EvUsageSimParams {
                mode,
                engage_charge_planning: false,
                weekday: day.clone(),
                weekend: day,
                min_soc_after_drop_pct: 5.0,
            }),
            usage_sim_seed_tag: 0,
        }
    }

    #[test]
    fn apply_usage_forecast_masks_the_predicted_away_window() {
        use crate::entities::asset_params::EvUsageMode;
        use chrono::{Duration, TimeZone, Timelike, Utc};
        let cfg = ev_with_usage(EvUsageMode::Forecast);
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 0, 0, 0).unwrap(); // Monday 00:00
        let n = 24;
        // n+1 boundaries, as `milp_planner::inputs` builds: cum_s[n] is the horizon
        // end. These fixtures used n entries, matching the forecast path's old
        // `cum_s[n-1]` read, which was a slot short of the real horizon.
        let cum_s: Vec<i64> = (0..=n as i64).map(|t| t * 3600).collect();

        let mut ctx = make_must_run(n); // starts fully available
        ctx.apply_usage_forecast(&cfg, n, &cum_s, now, &[]);

        // 08:00-17:00 away -> slots 8..=16 unavailable, the rest available.
        for (t, &ok) in ctx.a_ev.iter().enumerate() {
            let ts = now + Duration::hours(t as i64);
            let away = (8..17).contains(&ts.hour());
            assert_eq!(ok, !away, "slot {t} (hour {})", ts.hour());
        }
    }

    #[test]
    fn apply_usage_forecast_is_a_no_op_under_usage_sim_mode() {
        use crate::entities::asset_params::EvUsageMode;
        use chrono::{TimeZone, Utc};
        let cfg = ev_with_usage(EvUsageMode::Simulated); // same schedule, other class
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 0, 0, 0).unwrap();
        let n = 24;
        // n+1 boundaries, as `milp_planner::inputs` builds: cum_s[n] is the horizon
        // end. These fixtures used n entries, matching the forecast path's old
        // `cum_s[n-1]` read, which was a slot short of the real horizon.
        let cum_s: Vec<i64> = (0..=n as i64).map(|t| t * 3600).collect();

        let mut ctx = make_must_run(n);
        let before = ctx.a_ev.clone();
        ctx.apply_usage_forecast(&cfg, n, &cum_s, now, &[]);
        assert_eq!(
            ctx.a_ev, before,
            "usage_sim mode must leave the mask exactly as from_state built it"
        );
    }

    #[test]
    fn apply_usage_forecast_never_re_enables_a_masked_slot() {
        // Composition, not replacement: a slot already false (e.g. past a
        // session deadline) must stay false even where the EV is predicted home.
        use crate::entities::asset_params::EvUsageMode;
        use chrono::{TimeZone, Utc};
        let cfg = ev_with_usage(EvUsageMode::Forecast);
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 0, 0, 0).unwrap();
        let n = 24;
        // n+1 boundaries, as `milp_planner::inputs` builds: cum_s[n] is the horizon
        // end. These fixtures used n entries, matching the forecast path's old
        // `cum_s[n-1]` read, which was a slot short of the real horizon.
        let cum_s: Vec<i64> = (0..=n as i64).map(|t| t * 3600).collect();

        let mut ctx = make_must_run(n);
        ctx.a_ev = vec![false; n]; // everything already ruled out upstream
        ctx.apply_usage_forecast(&cfg, n, &cum_s, now, &[]);
        assert!(
            ctx.a_ev.iter().all(|&a| !a),
            "AND must never turn an unavailable slot back on"
        );
    }

    #[test]
    fn apply_usage_forecast_records_the_return_drop_for_the_projection() {
        use crate::entities::asset_params::EvUsageMode;
        use chrono::{TimeZone, Utc};
        let cfg = ev_with_usage(EvUsageMode::Forecast);
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 0, 0, 0).unwrap();
        let n = 24;
        // n+1 boundaries, as `milp_planner::inputs` builds: cum_s[n] is the horizon
        // end. These fixtures used n entries, matching the forecast path's old
        // `cum_s[n-1]` read, which was a slot short of the real horizon.
        let cum_s: Vec<i64> = (0..=n as i64).map(|t| t * 3600).collect();

        let mut ctx = make_must_run(n);
        ctx.apply_usage_forecast(&cfg, n, &cum_s, now, &[]);

        let drops = ctx.soc_drops.expect("forecast mode must record drops");
        assert_eq!(drops.drop_frac_per_slot.len(), n);
        assert!(
            (drops.floor_frac - 0.05).abs() < 1e-9,
            "floor must come from min_soc_after_drop_pct"
        );
        // Returns at exactly 17:00 and slot 17 starts at 17:00, so slot 17 is
        // the first slot "at or after" the return and carries the 20 % drop.
        let nonzero: Vec<usize> = (0..n)
            .filter(|&t| drops.drop_frac_per_slot[t] > 0.0)
            .collect();
        assert_eq!(
            nonzero,
            vec![17],
            "one drop, in the first slot at or after the return"
        );
        assert!((drops.drop_frac_per_slot[17] - 0.20).abs() < 1e-9);
    }

    #[test]
    fn apply_usage_forecast_records_no_drops_under_usage_sim_mode() {
        use crate::entities::asset_params::EvUsageMode;
        use chrono::{TimeZone, Utc};
        let cfg = ev_with_usage(EvUsageMode::Simulated);
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 0, 0, 0).unwrap();
        let n = 24;
        // n+1 boundaries, as `milp_planner::inputs` builds: cum_s[n] is the horizon
        // end. These fixtures used n entries, matching the forecast path's old
        // `cum_s[n-1]` read, which was a slot short of the real horizon.
        let cum_s: Vec<i64> = (0..=n as i64).map(|t| t * 3600).collect();

        let mut ctx = make_must_run(n);
        ctx.apply_usage_forecast(&cfg, n, &cum_s, now, &[]);
        assert!(
            ctx.soc_drops.is_none(),
            "usage_sim must not feed drops into the projection"
        );
    }

    /// A plugged EV with a `usage_forecast` schedule, built the way the planner
    /// builds it (no session anywhere), so `engage_charge_planning`'s effect is
    /// visible end to end.
    fn ctx_from_state(
        cfg: &super::EvCharger,
        n: usize,
        cum_s: &[i64],
        now: DateTime<Utc>,
    ) -> EvMilpContext {
        ctx_from_state_with_curve(cfg, n, cum_s, now, &[])
    }

    fn ctx_from_state_with_curve(
        cfg: &super::EvCharger,
        n: usize,
        cum_s: &[i64],
        now: DateTime<Utc>,
        comfort_rates: &[crate::entities::asset::ComfortRate],
    ) -> EvMilpContext {
        let state = super::super::AssetState::Ev(super::super::EvState {
            soc: 0.30,
            plugged: true,
            actual_power_kw: 0.0,
            pending_command_kw: 0.0,
            was_away_by_usage_sim: false,
        });
        let mut ctx = EvMilpContext::from_state(
            &state,
            cfg,
            n,
            cum_s,
            now,
            &[],
            comfort_rates,
            0.0,
            1.0,
            1.0,
            0.0,
            0.0,
            0.0,
        );
        ctx.apply_usage_forecast(cfg, n, cum_s, now, &[]);
        ctx
    }

    /// A forecast-driven charge must be priced by the user's comfort curve, exactly
    /// like a charge a user asked for.
    ///
    /// `engage_charge_planning` writes no `EvSession`, so before this test the
    /// forecast path set a target and a deadline but left `segments` empty — the
    /// curve was never consulted, and the energy beyond the target fell back to the
    /// flat `v_ev_extra_eur_kwh` reward, i.e. the pre-`ev-comfort-piecewise-core`
    /// model. Every fleet EV charges through this path, so the curve applied to
    /// nothing the fleet actually did (`no-half-built-features`).
    #[test]
    fn a_forecast_driven_charge_is_priced_by_the_comfort_curve() {
        use crate::entities::asset::ComfortRate;
        use crate::entities::asset_params::EvUsageMode;
        use chrono::{TimeZone, Utc};
        let mut cfg = ev_with_usage(EvUsageMode::Forecast);
        cfg.usage_sim.as_mut().unwrap().engage_charge_planning = true;
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 0, 0, 0).unwrap();
        let n = 24;
        // n+1 boundaries, as `milp_planner::inputs` builds: cum_s[n] is the horizon
        // end. These fixtures used n entries, matching the forecast path's old
        // `cum_s[n-1]` read, which was a slot short of the real horizon.
        let cum_s: Vec<i64> = (0..=n as i64).map(|t| t * 3600).collect();

        // A curve the user could have drawn: 0.50 falling to 0.10 as the pack fills.
        let curve = vec![
            ComfortRate {
                fill: 0.0,
                max_marginal_price: 0.50,
                max_marginal_co2: 0.0,
            },
            ComfortRate {
                fill: 1.0,
                max_marginal_price: 0.10,
                max_marginal_co2: 0.0,
            },
        ];
        let ctx = ctx_from_state_with_curve(&cfg, n, &cum_s, now, &curve);

        assert!(
            !ctx.segments.is_empty(),
            "a forecast-driven charge must carry priced bands, got none"
        );
        // Changed with the charge-limit fix, and the old number was the bug: this read
        // "bands must span soc_init..1.0 (42 kWh)", which is energy the charger refuses
        // at or above soc_target. The test's subject is the assertion below - that a
        // forecast-driven charge is priced by the user's curve rather than the profile
        // default - and that is untouched. Only the span is corrected:
        // soc 0.30 -> the 0.80 limit on a 60 kWh pack = 30 kWh.
        let total: f64 = ctx.segments.iter().map(|s| s.kwh).sum();
        assert!(
            (total - 30.0).abs() < 1e-6,
            "bands must span soc_init..soc_target (30 kWh), got {total}"
        );
        // Every bid must come from the curve's range, not from the profile default.
        for s in &ctx.segments {
            assert!(
                s.eur_per_kwh <= 0.50 + 1e-9 && s.eur_per_kwh >= 0.10 - 1e-9,
                "bid {} is outside the curve the user drew",
                s.eur_per_kwh
            );
        }
        // And the flat beyond-target reward must not also pay for the same energy.
        assert_eq!(
            ctx.e_extra_max_kwh, 0.0,
            "energy is priced by bands here, so `e_ev_extra` must be inert"
        );
    }

    /// Regression (found by `ev_usage_forecast.feature` running mid-trip): the
    /// car being unplugged RIGHT NOW must not blank the whole horizon when the
    /// EV has a usage forecast — the schedule says when it comes back, and the
    /// plan has to be able to charge in the slots after that.
    #[test]
    fn a_car_that_is_away_now_is_still_plannable_after_its_predicted_return() {
        use crate::entities::asset_params::EvUsageMode;
        use chrono::{TimeZone, Utc};
        let mut cfg = ev_with_usage(EvUsageMode::Forecast); // leaves 08:00, returns 17:00
        cfg.usage_sim.as_mut().unwrap().engage_charge_planning = true;
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 12, 0, 0).unwrap(); // mid-trip
        let n = 24;
        let cum_s: Vec<i64> = (0..=n as i64).map(|t| t * 3600).collect();

        let state = super::super::AssetState::Ev(super::super::EvState {
            soc: 0.30,
            plugged: false, // out driving
            actual_power_kw: 0.0,
            pending_command_kw: 0.0,
            was_away_by_usage_sim: true,
        });
        let mut ctx = EvMilpContext::from_state(
            &state,
            &cfg,
            n,
            &cum_s,
            now,
            &[],
            &[],
            0.0,
            1.0,
            1.0,
            0.0,
            0.0,
            0.0,
        );
        ctx.apply_usage_forecast(&cfg, n, &cum_s, now, &[]);

        assert!(
            !ctx.a_ev[0],
            "away right now: this slot really is unavailable"
        );
        assert!(
            !ctx.a_ev[4],
            "16:00 is still inside the predicted trip (returns 17:00)"
        );
        assert!(
            ctx.a_ev[6],
            "18:00 is after the predicted return — the plan must be able to charge"
        );
        assert_eq!(
            ctx.mode,
            EvMilpMode::MustRun,
            "the next departure still sets a target"
        );
        assert!(
            ctx.firm_required_kwh() > 1.0,
            "core energy must survive the reachability clamp, got {}",
            ctx.firm_required_kwh()
        );
    }

    #[test]
    fn a_car_that_is_away_now_stays_unplannable_under_usage_sim() {
        // usage_sim tells the planner nothing in advance, so the live plug is
        // still the only word on availability — today's behaviour, unchanged.
        use crate::entities::asset_params::EvUsageMode;
        use chrono::{TimeZone, Utc};
        let cfg = ev_with_usage(EvUsageMode::Simulated);
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 12, 0, 0).unwrap();
        let n = 24;
        let cum_s: Vec<i64> = (0..=n as i64).map(|t| t * 3600).collect();
        let state = super::super::AssetState::Ev(super::super::EvState {
            soc: 0.30,
            plugged: false,
            actual_power_kw: 0.0,
            pending_command_kw: 0.0,
            was_away_by_usage_sim: true,
        });
        let mut ctx = EvMilpContext::from_state(
            &state,
            &cfg,
            n,
            &cum_s,
            now,
            &[],
            &[],
            0.0,
            1.0,
            1.0,
            0.0,
            0.0,
            0.0,
        );
        ctx.apply_usage_forecast(&cfg, n, &cum_s, now, &[]);
        assert!(
            ctx.a_ev.iter().all(|&a| !a),
            "no forecast: an unplugged EV contributes nothing"
        );
    }

    #[test]
    fn engage_charge_planning_targets_the_next_predicted_departure() {
        use crate::entities::asset_params::EvUsageMode;
        use chrono::{TimeZone, Utc};
        let mut cfg = ev_with_usage(EvUsageMode::Forecast);
        cfg.usage_sim.as_mut().unwrap().engage_charge_planning = true;
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 0, 0, 0).unwrap();
        let n = 24;
        // n+1 boundaries, as `milp_planner::inputs` builds: cum_s[n] is the horizon
        // end. These fixtures used n entries, matching the forecast path's old
        // `cum_s[n-1]` read, which was a slot short of the real horizon.
        let cum_s: Vec<i64> = (0..=n as i64).map(|t| t * 3600).collect();

        let ctx = ctx_from_state(&cfg, n, &cum_s, now);
        assert_eq!(
            ctx.mode,
            EvMilpMode::MustRun,
            "a target must be planned for"
        );
        // Derived from the trip rather than hard-coded. The departure carries +/-10 min
        // of jitter, so whether 08:00 lands in slot 7 or 8 is a property of the seed,
        // not of the behaviour under test - and a literal 8 asserts the seed. What is
        // actually required is that the deadline is the slot the departure falls in:
        // its window must bracket the departure, so being ready by it means being ready
        // before the car leaves.
        let trip = crate::assets::ev_schedule::next_trip_after(
            cfg.usage_sim.as_ref().unwrap(),
            cfg.usage_sim_seed_tag,
            now,
            now + chrono::Duration::seconds(cum_s[n]),
        )
        .expect("a trip inside the horizon");
        let step = ctx.obligations[0].deadline_step;
        let slot_end = now + chrono::Duration::seconds(cum_s[step + 1]);
        let slot_start = now + chrono::Duration::seconds(cum_s[step]);
        assert!(
            slot_start < trip.leave_at && trip.leave_at <= slot_end,
            "deadline slot {step} [{slot_start}, {slot_end}] must bracket the departure {}",
            trip.leave_at
        );
        // soc 0.30 -> soc_target 0.80 over a 60 kWh pack = 30 kWh.
        assert!(
            (ctx.firm_required_kwh() - 30.0).abs() < 1e-9,
            "core energy must target soc_target by departure, got {}",
            ctx.firm_required_kwh()
        );
        // 8 h at 7.4 kW covers 30 kWh, so the floor is the whole requirement.
        let dt_h = vec![1.0; n];
        let deadline = ctx.obligations[0].deadline_step;
        assert!(
            window_energy_kwh(&ctx, n, &dt_h, deadline) >= ctx.firm_required_kwh(),
            "the window must be able to hold the requirement"
        );
    }

    /// `ev-comfort-piecewise-core`: the requirement stays what the user asked
    /// for — it is no longer shrunk to fit. What gets capped is the MILP *floor*
    /// (in `constraints`), so the solve stays feasible; the gap between the two
    /// is what `ev_diagnostics::firm_shortfall` reports.
    #[test]
    fn a_target_the_remaining_window_cannot_reach_keeps_its_requirement() {
        use crate::entities::asset_params::EvUsageMode;
        use chrono::{TimeZone, Utc};
        let mut cfg = ev_with_usage(EvUsageMode::Forecast);
        cfg.usage_sim.as_mut().unwrap().engage_charge_planning = true;
        // 06:00: only slots 0 and 1 remain before the 08:00 departure, i.e.
        // 2 h x 7.4 kW = 14.8 kWh against a 30 kWh target.
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 6, 0, 0).unwrap();
        let n = 24;
        let cum_s: Vec<i64> = (0..=n as i64).map(|t| t * 3600).collect();

        let ctx = ctx_from_state(&cfg, n, &cum_s, now);
        // 1, not 2: a departure landing exactly on a boundary must be met by the slot
        // that ENDS there. 049 corrected this for stated sessions and left the forecast
        // path on the old at-or-before rule; one shared helper now applies it to both.
        // The window below is unchanged - slots 0 and 1, 14.8 kWh - because it is
        // inclusive and `a_ev`-filtered, so slot 2 was never counted anyway.
        assert_eq!(ctx.obligations[0].deadline_step, 1);
        assert!(
            (ctx.firm_required_kwh() - 30.0).abs() < 1e-6,
            "the requirement is what the user asked for, got {}",
            ctx.firm_required_kwh()
        );
        let dt_h = vec![1.0; n];
        let deadline = ctx.obligations[0].deadline_step;
        assert!(
            (window_energy_kwh(&ctx, n, &dt_h, deadline) - 14.8).abs() < 1e-6,
            "only slots 0 and 1 are home before the 08:00 departure — \
             2 h at 7.4 kW = 14.8 kWh, got {}",
            window_energy_kwh(&ctx, n, &dt_h, deadline)
        );
    }

    #[test]
    fn engage_charge_planning_off_introduces_no_target_but_keeps_availability() {
        use crate::entities::asset_params::EvUsageMode;
        use chrono::{TimeZone, Utc};
        let cfg = ev_with_usage(EvUsageMode::Forecast); // engage_charge_planning: false
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 0, 0, 0).unwrap();
        let n = 24;
        // n+1 boundaries, as `milp_planner::inputs` builds: cum_s[n] is the horizon
        // end. These fixtures used n entries, matching the forecast path's old
        // `cum_s[n-1]` read, which was a slot short of the real horizon.
        let cum_s: Vec<i64> = (0..=n as i64).map(|t| t * 3600).collect();

        let ctx = ctx_from_state(&cfg, n, &cum_s, now);
        assert!(
            ctx.obligations.is_empty(),
            "no obligation may be introduced"
        );
        assert_eq!(
            ctx.firm_required_kwh(),
            0.0,
            "no core obligation may be introduced"
        );
        // Availability is unconditional — the away window is still masked.
        assert!(
            !ctx.a_ev[10],
            "a predicted-away slot must stay unavailable even with no target"
        );
        assert!(ctx.a_ev[0], "and a predicted-home slot must stay available");
    }

    #[test]
    fn a_real_session_target_wins_over_the_forecasts() {
        use crate::entities::asset_params::EvUsageMode;
        use crate::entities::device_session::{EvSession, EvSessionOrigin};
        use chrono::{Duration as ChronoDuration, TimeZone, Utc};
        let mut cfg = ev_with_usage(EvUsageMode::Forecast);
        cfg.usage_sim.as_mut().unwrap().engage_charge_planning = true;
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 0, 0, 0).unwrap();
        let n = 24;
        // n+1 boundaries, as `milp_planner::inputs` builds: cum_s[n] is the horizon
        // end. These fixtures used n entries, matching the forecast path's old
        // `cum_s[n-1]` read, which was a slot short of the real horizon.
        let cum_s: Vec<i64> = (0..=n as i64).map(|t| t * 3600).collect();

        // A real request: 40 % by 04:00 — a different target AND deadline than
        // the forecast's (80 % by 08:00).
        let session = EvSession {
            id: uuid::Uuid::new_v4(),
            target_soc: 0.40,
            window_start: now,
            expected_trip_distance_km: None,
            expected_return_time: None,
            departure_time: now + ChronoDuration::hours(4),
            soft_deadline: false,
            mode: crate::entities::design_vocabulary::UserRequestMode::ByDeadline,
            origin: EvSessionOrigin::UserRequest,
            budget_eur: None,
            comfort_rates: vec![],
            created_at: now,
            updated_at: now,
        };
        let state = super::super::AssetState::Ev(super::super::EvState {
            soc: 0.30,
            plugged: true,
            actual_power_kw: 0.0,
            pending_command_kw: 0.0,
            was_away_by_usage_sim: false,
        });
        let mut ctx = EvMilpContext::from_state(
            &state,
            &cfg,
            n,
            &cum_s,
            now,
            std::slice::from_ref(&session),
            &[],
            0.0,
            1.0,
            1.0,
            0.0,
            0.0,
            0.0,
        );
        ctx.apply_usage_forecast(&cfg, n, &cum_s, now, std::slice::from_ref(&session));

        // Departure is exactly 4 h on an hourly grid, so the last chargeable slot
        // is 3: slot 4 runs [4 h, 5 h), entirely after the car has left. (Was 4,
        // the same boundary off-by-one corrected in `slot_at`.) What this test is
        // about is unchanged: the stated session's deadline wins over the
        // forecast's, whatever slot that lands on.
        assert_eq!(
            ctx.obligations[0].deadline_step, 3,
            "the real session's deadline wins"
        );
        assert!(
            (ctx.firm_required_kwh() - 6.0).abs() < 1e-9,
            "the real session's target wins (0.40-0.30)*60 = 6 kWh, got {}",
            ctx.firm_required_kwh()
        );
        // ...but availability is still the forecast's, not the session's guess.
        assert!(
            !ctx.a_ev[10],
            "a predicted-away slot stays unavailable even under a real session"
        );
    }

    /// The drops must survive the hop the planner actually uses:
    /// context -> `milp_params` (EvScalars) -> MilpInputs -> `results.rs`.
    #[test]
    fn milp_params_carries_soc_drops_through_to_the_scalars() {
        use crate::controller::milp_planner::AssetMilpParams;
        use crate::entities::asset_params::EvUsageMode;
        use chrono::{TimeZone, Utc};
        let cfg = ev_with_usage(EvUsageMode::Forecast);
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 0, 0, 0).unwrap();
        let n = 24;
        // n+1 boundaries, as `milp_planner::inputs` builds: cum_s[n] is the horizon
        // end. These fixtures used n entries, matching the forecast path's old
        // `cum_s[n-1]` read, which was a slot short of the real horizon.
        let cum_s: Vec<i64> = (0..=n as i64).map(|t| t * 3600).collect();

        let mut ctx = make_must_run(n);
        ctx.apply_usage_forecast(&cfg, n, &cum_s, now, &[]);
        let AssetMilpParams::Ev(scalars) =
            crate::controller::milp_planner::AssetMilpContext::milp_params(&ctx, n, now)
        else {
            panic!("expected Ev params");
        };
        assert_eq!(
            scalars.soc_drops, ctx.soc_drops,
            "milp_params must pass the drops through, not drop them"
        );
    }

    /// What the unmasked slots before `deadline_step` can physically deliver [kWh].
    ///
    /// R-93 deleted the production `reachable_energy_kwh`: the solver now decides
    /// how much of an obligation it can meet and reports the rest as shortfall
    /// slack, so nothing outside the model needs to pre-compute this. The two tests
    /// below still assert the *window* arithmetic they always did, so the rule
    /// lives here, in the only place that still asks the question. That an
    /// unreachable target degrades to a reported gap rather than an infeasible
    /// solve is covered by
    /// `milp_planner::tests::soc_balance::an_unreachable_obligation_reports_the_gap_instead_of_failing_the_solve`.
    fn window_energy_kwh(ctx: &EvMilpContext, n: usize, dt_h: &[f64], deadline_step: usize) -> f64 {
        dt_h.iter()
            .enumerate()
            .take(n)
            .filter(|&(t, _)| t <= deadline_step && ctx.a_ev.get(t).copied().unwrap_or(false))
            .map(|(_, &dt)| ctx.p_max_kw * dt)
            .sum()
    }

    use crate::controller::milp_planner::asset_port::EvObligation;

    fn make_must_run(n: usize) -> EvMilpContext {
        EvMilpContext {
            mode: EvMilpMode::MustRun,
            soc_init: 0.0,
            soc_max: 1.0,
            a_ev: vec![true; n],
            soc_drops: None,
            obligations: vec![EvObligation {
                deadline_step: n - 1,
                // 10 kWh of a 60 kWh pack from empty — what this fixture used to
                // state as `e_required_kwh: 10.0`.
                target_soc: 10.0 / 60.0,
                session_id: None,
            }],
            battery_kwh: 60.0,
            p_max_kw: 7.2,
            p_min_kw: 0.0,
            segments: vec![],
            e_extra_max_kwh: 5.0,
            v_extra_eur_kwh: 0.05,
            asap_lateness_eur_kwh_h: 0.0,
            free_only: false,
            p_free_cap_kw: None,
            reward_per_slot: false,
            free_early_bias: false,
            budget_eur: None,
            c_imp_eur_kwh: None,
            v_extra_co2_eur_kwh: 0.0,
        }
    }

    /// BL-34, restated for `ev-comfort-piecewise-core`: the `ByDeadline` arm prices its
    /// energy bands from the session's own comfort curve, not from the passed-in global
    /// defaults. The curve here bids 0.05 €/kWh, the defaults are 1.0 — if the curve were
    /// ignored the bands would carry 1.0.
    #[test]
    fn from_state_by_deadline_soft_prices_segments_from_the_curve() {
        use crate::entities::asset::ComfortRate;
        use crate::entities::design_vocabulary::UserRequestMode;
        use crate::entities::device_session::EvSession;
        use chrono::Utc;

        let cfg = super::EvCharger {
            max_charge_kw: 7.4,
            max_discharge_kw: 0.0,
            v2g_capable: false,
            battery_kwh: 60.0,
            consumption_kwh_per_km: 0.18,
            soc_target: 0.8,
            soc_target_profile: 0.8,
            default_charge_kw: 7.4,
            min_soc: 0.0,
            min_charge_kw: 0.0,
            response_delay_s: 0.0,
            departure_time: None,
            usage_sim: None,
            usage_sim_seed_tag: 0,
        };
        let state = super::super::AssetState::Ev(super::super::EvState {
            soc: 0.2,
            plugged: true,
            actual_power_kw: 0.0,
            pending_command_kw: 0.0,
            was_away_by_usage_sim: false,
        });
        let now = Utc::now();
        let session = EvSession {
            id: uuid::Uuid::new_v4(),
            target_soc: 0.3,
            window_start: Utc::now(),
            expected_trip_distance_km: None,
            expected_return_time: None,
            departure_time: now + chrono::Duration::hours(2),
            soft_deadline: true,
            mode: UserRequestMode::ByDeadline,
            origin: crate::entities::device_session::EvSessionOrigin::UserRequest,
            budget_eur: None,
            comfort_rates: vec![
                ComfortRate {
                    fill: 0.0,
                    max_marginal_price: 0.05,
                    max_marginal_co2: 0.0,
                },
                ComfortRate {
                    fill: 1.0,
                    max_marginal_price: 0.12,
                    max_marginal_co2: 0.0,
                },
            ],
            created_at: now,
            updated_at: now,
        };
        let cum_s: Vec<i64> = (0..=24).map(|i| i * 300).collect();
        let ctx = EvMilpContext::from_state(
            &state,
            &cfg,
            24,
            &cum_s,
            now,
            std::slice::from_ref(&session),
            &[],
            0.0,
            1.0,
            1.0,
            0.0,
            0.0,
            0.0,
        );
        // A soft deadline guarantees nothing — the bands carry the intent.
        assert_eq!(ctx.firm_required_kwh(), 0.0);
        let total_kwh: f64 = ctx.segments.iter().map(|s| s.kwh).sum();
        assert!(
            total_kwh > 6.0,
            "bands must span past the target to full, got {total_kwh}"
        );
        assert!(!ctx.segments.is_empty(), "the curve must produce bands");
        for seg in &ctx.segments {
            assert!(
                seg.eur_per_kwh <= 0.12 + 1e-9,
                "bands must carry the curve's own bids (0.05..0.12), not the 1.0 default: {:?}",
                ctx.segments
            );
        }
        // A soft deadline guarantees nothing: the bids decide.
        assert_eq!(ctx.firm_required_kwh(), 0.0);
        assert_eq!(ctx.mode, EvMilpMode::MayRun);
    }

    /// The planner seeds the EV's SoC forecast from the EV's own MILP params,
    /// not from a raw snapshot read — for every branch, session or not.
    #[test]
    fn milp_params_report_the_live_soc_in_every_branch() {
        let cfg = super::EvCharger {
            max_charge_kw: 7.4,
            max_discharge_kw: 0.0,
            v2g_capable: false,
            battery_kwh: 60.0,
            consumption_kwh_per_km: 0.18,
            soc_target: 0.8,
            soc_target_profile: 0.8,
            default_charge_kw: 7.4,
            min_soc: 0.0,
            min_charge_kw: 0.0,
            response_delay_s: 0.0,
            departure_time: None,
            usage_sim: None,
            usage_sim_seed_tag: 0,
        };
        let cum_s: Vec<i64> = (0..=4).map(|i| i * 300).collect();
        for plugged in [true, false] {
            let state = super::super::AssetState::Ev(super::super::EvState {
                soc: 0.42,
                plugged,
                actual_power_kw: 0.0,
                pending_command_kw: 0.0,
                was_away_by_usage_sim: false,
            });
            let ctx = EvMilpContext::from_state(
                &state,
                &cfg,
                4,
                &cum_s,
                chrono::Utc::now(),
                &[],
                &[],
                0.0,
                1.0,
                1.0,
                0.0,
                0.0,
                0.0,
            );
            match ctx.milp_params(4, chrono::Utc::now()) {
                AssetMilpParams::Ev(e) => assert_eq!(e.soc_init, 0.42, "plugged={plugged}"),
                _ => panic!("expected Ev variant"),
            }
        }
    }

    /// Empty `comfort_rates` (legacy `/ev-session` route, VTN-commanded sessions) falls back
    /// to the passed-in global defaults exactly — no panic, no behavior change.
    #[test]
    fn from_state_by_deadline_empty_curve_falls_back_to_global_defaults() {
        use crate::entities::design_vocabulary::UserRequestMode;
        use crate::entities::device_session::EvSession;
        use chrono::Utc;

        let cfg = super::EvCharger {
            max_charge_kw: 7.4,
            max_discharge_kw: 0.0,
            v2g_capable: false,
            battery_kwh: 60.0,
            consumption_kwh_per_km: 0.18,
            soc_target: 0.8,
            soc_target_profile: 0.8,
            default_charge_kw: 7.4,
            min_soc: 0.0,
            min_charge_kw: 0.0,
            response_delay_s: 0.0,
            departure_time: None,
            usage_sim: None,
            usage_sim_seed_tag: 0,
        };
        let state = super::super::AssetState::Ev(super::super::EvState {
            soc: 0.2,
            plugged: true,
            actual_power_kw: 0.0,
            pending_command_kw: 0.0,
            was_away_by_usage_sim: false,
        });
        let now = Utc::now();
        let session = EvSession {
            id: uuid::Uuid::new_v4(),
            target_soc: 0.3,
            window_start: now,
            expected_trip_distance_km: None,
            expected_return_time: None,
            departure_time: now + chrono::Duration::hours(2),
            soft_deadline: true,
            mode: UserRequestMode::ByDeadline,
            origin: crate::entities::device_session::EvSessionOrigin::UserRequest,
            budget_eur: None,
            comfort_rates: vec![],
            created_at: now,
            updated_at: now,
        };
        let cum_s: Vec<i64> = (0..=24).map(|i| i * 300).collect();
        let ctx = EvMilpContext::from_state(
            &state,
            &cfg,
            24,
            &cum_s,
            now,
            std::slice::from_ref(&session),
            &[],
            0.0,
            0.42, // v_ev_extra_eur_kwh
            0.77, // v_ev_core_eur_kwh
            0.0,
            0.0,
            0.0,
        );
        // No curve on the session: the profile defaults stand in as a two-step
        // curve — 0.77 €/kWh up to the target, 0.42 €/kWh beyond it.
        let bids: Vec<f64> = ctx.segments.iter().map(|s| s.eur_per_kwh).collect();
        assert!(
            bids.iter().any(|b| (b - 0.77).abs() < 1e-9),
            "expected the core default 0.77 below the target: {bids:?}"
        );
        assert!(
            bids.iter().any(|b| (b - 0.42).abs() < 1e-9),
            "expected the extra default 0.42 above it: {bids:?}"
        );
    }

    #[test]
    fn asset_id_is_ev() {
        assert_eq!(make_must_run(4).asset_id(), "ev");
    }

    #[test]
    fn asset_kind_is_ev() {
        assert_eq!(make_must_run(4).asset_kind(), AssetKind::Ev);
    }

    #[test]
    fn milp_params_must_run_mode() {
        let ctx = make_must_run(4);
        match ctx.milp_params(4, chrono::Utc::now()) {
            AssetMilpParams::Ev(e) => assert_eq!(e.mode, MilpLoadMode::MustRun),
            _ => panic!("expected Ev variant"),
        }
    }

    #[test]
    fn milp_params_may_run_mode() {
        let ctx = EvMilpContext {
            mode: EvMilpMode::MayRun,
            soc_init: 0.0,
            soc_max: 1.0,
            a_ev: vec![true; 4],
            soc_drops: None,
            obligations: vec![],
            battery_kwh: 60.0,
            p_max_kw: 7.2,
            p_min_kw: 0.0,
            segments: vec![],
            e_extra_max_kwh: 5.0,
            v_extra_eur_kwh: 0.05,
            asap_lateness_eur_kwh_h: 0.0,
            free_only: false,
            p_free_cap_kw: None,
            reward_per_slot: false,
            free_early_bias: false,
            budget_eur: None,
            c_imp_eur_kwh: None,
            v_extra_co2_eur_kwh: 0.0,
        };
        match ctx.milp_params(4, chrono::Utc::now()) {
            AssetMilpParams::Ev(e) => assert_eq!(e.mode, MilpLoadMode::MayRun),
            _ => panic!("expected Ev variant"),
        }
    }

    #[test]
    fn milp_params_must_not_run_mode() {
        let ctx = EvMilpContext {
            mode: EvMilpMode::MustNotRun,
            soc_init: 0.0,
            soc_max: 1.0,
            a_ev: vec![false; 4],
            soc_drops: None,
            obligations: vec![],
            battery_kwh: 60.0,
            p_max_kw: 7.2,
            p_min_kw: 0.0,
            segments: vec![],
            e_extra_max_kwh: 5.0,
            v_extra_eur_kwh: 0.05,
            asap_lateness_eur_kwh_h: 0.0,
            free_only: false,
            p_free_cap_kw: None,
            reward_per_slot: false,
            free_early_bias: false,
            budget_eur: None,
            c_imp_eur_kwh: None,
            v_extra_co2_eur_kwh: 0.0,
        };
        match ctx.milp_params(4, chrono::Utc::now()) {
            AssetMilpParams::Ev(e) => assert_eq!(e.mode, MilpLoadMode::MustNotRun),
            _ => panic!("expected Ev variant"),
        }
    }

    #[test]
    fn milp_params_propagates_a_ev() {
        let n = 4;
        let a_ev = vec![true, false, true, false];
        let ctx = EvMilpContext {
            mode: EvMilpMode::MayRun,
            soc_init: 0.0,
            soc_max: 1.0,
            a_ev: a_ev.clone(),
            soc_drops: None,
            obligations: vec![],
            battery_kwh: 60.0,
            p_max_kw: 7.2,
            p_min_kw: 0.0,
            segments: vec![],
            e_extra_max_kwh: 5.0,
            v_extra_eur_kwh: 0.05,
            asap_lateness_eur_kwh_h: 0.0,
            free_only: false,
            p_free_cap_kw: None,
            reward_per_slot: false,
            free_early_bias: false,
            budget_eur: None,
            c_imp_eur_kwh: None,
            v_extra_co2_eur_kwh: 0.0,
        };
        match ctx.milp_params(n, chrono::Utc::now()) {
            AssetMilpParams::Ev(e) => assert_eq!(e.a_ev, a_ev),
            _ => panic!("expected Ev variant"),
        }
    }

    #[test]
    fn declare_vars_fills_pool_ev_slot() {
        let n = 4;
        let ctx = make_must_run(n);
        let mut vars = variables!();
        let mut pool = empty_pool(&mut vars, n);
        ctx.declare_vars_into_pool(n, 0.0, 0.0, &mut vars, &mut pool);
        let v = pool
            .ev
            .as_ref()
            .expect("pool.ev should be Some after declare");
        assert_eq!(v.p_ev.len(), n);
        assert_eq!(v.z_ev_on.len(), n);
        assert!(v.delta_ev.is_empty()); // no startup vars when c_startup=0
    }

    /// R-98: the marginal-cost pass declares the EV through the plan's own declaration —
    /// the guarantee band and the SoC ceiling included — with `z_ev_on` fixed.
    #[test]
    fn declare_pinned_vars_is_the_free_declaration_with_z_ev_on_fixed() {
        use crate::controller::milp_planner::asset_port::WinningModeDecisions;
        let n = 4;
        let ctx = make_must_run(n);
        let winning = WinningModeDecisions {
            z_ev_on: vec![0.0, 1.0, 0.8, 0.1],
            ..Default::default()
        };
        let shape = |pinned: bool| {
            let mut vars = variables!();
            let mut pool = empty_pool(&mut vars, n);
            if pinned {
                ctx.declare_pinned_vars_into_pool(n, &winning, &mut vars, &mut pool);
            } else {
                ctx.declare_vars_into_pool(n, 0.0, 0.0, &mut vars, &mut pool);
            }
            let v = pool.ev.expect("EV declared");
            (
                v.p_ev.len(),
                v.soc_ev.len(),
                v.drop_unmet.len(),
                v.shortfall_soc.len(),
                v.z_ev_on.len(),
                v.e_seg.len(),
            )
        };
        assert_eq!(shape(true), shape(false));

        let range = bound_range(|vars| {
            let mut pool = empty_pool(vars, n);
            ctx.declare_pinned_vars_into_pool(n, &winning, vars, &mut pool);
            pool.ev.expect("EV declared").z_ev_on
        });
        assert_eq!(range, vec![(0.0, 0.0), (1.0, 1.0), (1.0, 1.0), (0.0, 0.0)]);
    }
}
