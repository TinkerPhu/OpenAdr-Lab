// ── EV MILP plugin types ──────────────────────────────────────────────────────
// Struct/enum definitions live in `controller::milp_planner::asset_port`.
// Method implementations below are cross-file inherent impl blocks — valid Rust.

use chrono::{DateTime, Utc};
use good_lp::{constraint, variable, Constraint, Expression, ProblemVariables, Solution};

use super::EvCharger;
use crate::controller::milp_planner::asset_port::{
    EvMilpContext, EvMilpMode, EvMilpVars, EvSolOutput,
};

/// WP4.1-c MAX_COST: per-kWh completion reward - an order of magnitude above
/// any real tariff so the solver charges toward the target regardless of
/// price, with the budget constraint (not the price) doing the capping.
const BUDGET_CHARGE_REWARD_EUR_KWH: f64 = 5.0;

impl EvMilpContext {
    /// Declare all LP variables for this EV charger. Context-side canonical implementation.
    pub fn declare_vars(
        &self,
        n: usize,
        c_startup_eur: f64,
        c_ramp_eur_kw: f64,
        vars: &mut ProblemVariables,
    ) -> EvMilpVars {
        let p_ev = (0..n)
            .map(|_| {
                if self.mode == EvMilpMode::MustNotRun {
                    vars.add(variable().min(0.0).max(0.0))
                } else {
                    vars.add(variable().min(0.0).max(self.p_max_kw))
                }
            })
            .collect();
        let z_ev_on = (0..n)
            .map(|t| {
                if self.mode == EvMilpMode::MustNotRun {
                    vars.add(variable().min(0.0).max(0.0))
                } else {
                    let ub = if self.a_ev[t] { 1.0 } else { 0.0 };
                    vars.add(variable().max(ub).binary())
                }
            })
            .collect();
        let z_ev_core = if self.mode == EvMilpMode::MayRun {
            vars.add(variable().binary())
        } else {
            vars.add(variable().min(0.0).max(0.0))
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
            z_ev_on,
            z_ev_core,
            e_ev_extra,
            delta_ev,
            delta_ev_ramp,
            p_min_kw: self.p_min_kw,
        }
    }

    /// Build the energy accumulator expression up to the deadline step.
    /// `dt_h[t]` is the slot duration in hours for slot `t`.
    pub fn energy_expr(&self, v: &EvMilpVars, n: usize, dt_h: &[f64]) -> Expression {
        let t_dlim = self.t_dead_step.unwrap_or(n.saturating_sub(1));
        let mut expr = Expression::from(0.0);
        for (t, &dt) in dt_h.iter().enumerate().take(n) {
            if t <= t_dlim {
                expr += dt * v.p_ev[t];
            }
        }
        expr
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
        match self.mode {
            EvMilpMode::MustRun => {
                cs.push(constraint!(ev_energy.clone() >= self.e_core_kwh));
                cs.push(constraint!(ev_energy <= self.e_core_kwh + v.e_ev_extra));
            }
            EvMilpMode::MayRun => {
                cs.push(constraint!(
                    ev_energy.clone() >= self.e_core_kwh * v.z_ev_core
                ));
                cs.push(constraint!(
                    ev_energy <= self.e_core_kwh * v.z_ev_core + v.e_ev_extra
                ));
                cs.push(constraint!(
                    v.e_ev_extra <= self.e_extra_max_kwh * v.z_ev_core
                ));
            }
            EvMilpMode::MustNotRun => {}
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
        }
        if self.mode == EvMilpMode::MayRun {
            obj += -(w_services * self.v_core_eur) * v.z_ev_core;
        }
        // WP4.1 (BL-28) OPPORTUNISTIC / *_FREE / MAX_COST: reward the energy
        // actually charged, per slot. The e_ev_extra reward above cannot drive
        // charging — e_ev_extra only *bounds* energy from above (ev_energy ≤
        // core + e_ev_extra), so the solver banks that reward without moving
        // p_ev. ASAP_FREE additionally biases the reward toward earlier slots
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
        if self.asap_lateness_eur_kwh_h > 0.0 && self.mode != EvMilpMode::MustNotRun {
            let mut elapsed_h = 0.0;
            for (t, &dt) in dt_h.iter().enumerate().take(n) {
                let mid_h = elapsed_h + dt / 2.0;
                obj += (self.asap_lateness_eur_kwh_h * mid_h * dt) * v.p_ev[t];
                elapsed_h += dt;
            }
        }
        obj
    }

    /// Read back the EV solution. Associated function (no `self` needed).
    pub fn read_solution(sol: &impl Solution, v: &EvMilpVars, n: usize) -> EvSolOutput {
        EvSolOutput {
            p_ev_kw: (0..n).map(|t| sol.value(v.p_ev[t])).collect(),
            z_ev_on: (0..n).map(|t| sol.value(v.z_ev_on[t])).collect(),
            e_ev_extra_kwh: sol.value(v.e_ev_extra),
            z_ev_core: sol.value(v.z_ev_core),
        }
    }

    /// Construct from a live `AssetState`, sim `EvCharger` config, and optional session data.
    #[allow(clippy::too_many_arguments)]
    pub fn from_state(
        state: &super::AssetState,
        cfg: &EvCharger,
        n: usize,
        cum_s: &[i64],
        now: DateTime<Utc>,
        ev_session: Option<&crate::entities::device_session::EvSession>,
        min_charge_kw: f64,
        v_ev_extra_eur_kwh: f64,
        v_ev_core_eur_kwh: f64,
        asap_lateness_eur_kwh_h: f64,
        v_ev_free_charge_eur_kwh: f64,
    ) -> Self {
        use crate::entities::design_vocabulary::UserRequestMode;
        let plugged = if let super::AssetState::Ev(s) = state {
            s.plugged
        } else {
            false
        };
        // Idle/unplugged template — every branch below overrides only what differs.
        let base = Self {
            mode: EvMilpMode::MustNotRun,
            a_ev: vec![false; n],
            t_dead_step: None,
            p_max_kw: cfg.max_charge_kw,
            p_min_kw: min_charge_kw,
            e_core_kwh: 0.0,
            e_extra_max_kwh: cfg.battery_kwh * (1.0 - cfg.soc_target),
            v_extra_eur_kwh: v_ev_extra_eur_kwh,
            v_core_eur: 0.0,
            asap_lateness_eur_kwh_h: 0.0,
            free_only: false,
            p_free_cap_kw: None,
            reward_per_slot: false,
            free_early_bias: false,
            budget_eur: None,
            c_imp_eur_kwh: None,
        };
        if !plugged {
            return base;
        }
        let Some(session) = ev_session else {
            // Plugged, no session: slots available but no charging obligation.
            return Self {
                a_ev: vec![true; n],
                ..base
            };
        };
        let current_soc = if let super::AssetState::Ev(s) = state {
            s.soc
        } else {
            0.0
        };
        let core_kwh = ((session.target_soc - current_soc) * cfg.battery_kwh).max(0.0);
        let secs = (session.departure_time - now).num_seconds();
        let t_dead = if secs <= 0 {
            0
        } else {
            cum_s
                .partition_point(|&s| s <= secs)
                .saturating_sub(1)
                .min(n.saturating_sub(1))
        };
        let deadline_mask: Vec<bool> = (0..n).map(|t| t <= t_dead).collect();

        match session.mode {
            // WP4.1 (BL-28) OPPORTUNISTIC / ASAP_FREE: no deadline, no core
            // obligation - all charging is optional "extra" up to the session
            // target, rewarded per charged kWh but gated to free energy via
            // inject_grid_slots. ASAP_FREE additionally biases the reward
            // toward earlier slots.
            UserRequestMode::Opportunistic | UserRequestMode::AsapFree => Self {
                mode: EvMilpMode::MustRun, // core = 0 -> only the gated extra term acts
                a_ev: vec![true; n],
                e_extra_max_kwh: core_kwh,
                v_extra_eur_kwh: v_ev_free_charge_eur_kwh,
                free_only: true,
                reward_per_slot: true,
                free_early_bias: session.mode == UserRequestMode::AsapFree,
                ..base
            },
            // WP4.1-c MAX_COST: complete whenever, but total charging cost
            // stays within the budget (hard constraint from the injected
            // import rates). Completion is a per-kWh reward high enough to
            // beat any real tariff, NOT a hard core constraint - an
            // unaffordable target degrades to partial charging + a plan
            // warning, never an infeasible solve.
            UserRequestMode::MaxCost => Self {
                mode: EvMilpMode::MustRun,
                a_ev: vec![true; n],
                e_extra_max_kwh: core_kwh,
                v_extra_eur_kwh: BUDGET_CHARGE_REWARD_EUR_KWH,
                reward_per_slot: true,
                budget_eur: session.budget_eur,
                ..base
            },
            // WP4.1-c BY_DEADLINE_FREE: the deadline mask stays, but there is
            // no core obligation (free energy may simply not exist) - free-
            // gated per-kWh reward inside the window instead.
            UserRequestMode::ByDeadlineFree => Self {
                mode: EvMilpMode::MustRun,
                a_ev: deadline_mask,
                t_dead_step: Some(t_dead),
                e_extra_max_kwh: core_kwh,
                v_extra_eur_kwh: v_ev_free_charge_eur_kwh,
                free_only: true,
                reward_per_slot: true,
                ..base
            },
            // Legacy BY_DEADLINE (+ ASAP, which only adds the lateness
            // penalty): hard/soft core energy by the departure deadline.
            UserRequestMode::ByDeadline | UserRequestMode::Asap => Self {
                mode: if session.soft_deadline {
                    EvMilpMode::MayRun
                } else {
                    EvMilpMode::MustRun
                },
                a_ev: deadline_mask,
                t_dead_step: Some(t_dead),
                e_core_kwh: core_kwh,
                e_extra_max_kwh: cfg.battery_kwh * (1.0 - session.target_soc),
                v_core_eur: if session.soft_deadline {
                    core_kwh * v_ev_core_eur_kwh
                } else {
                    0.0
                },
                asap_lateness_eur_kwh_h: if session.mode == UserRequestMode::Asap {
                    asap_lateness_eur_kwh_h
                } else {
                    0.0
                },
                ..base
            },
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
                a_ev: self.a_ev.clone(),
                t_dead_step: self.t_dead_step,
                p_max_kw: self.p_max_kw,
                p_min_kw: self.p_min_kw,
                e_core_kwh: self.e_core_kwh,
                e_extra_max_kwh: self.e_extra_max_kwh,
                v_extra_eur_kwh: self.v_extra_eur_kwh,
                v_core_eur: self.v_core_eur,
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

    fn constraints(
        &self,
        pool: &crate::controller::milp_interactions::MilpVarPool,
        n: usize,
        dt_h: &[f64],
    ) -> Vec<Constraint> {
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
    use crate::controller::milp_interactions::{GridMilpVars, MilpVarPool};
    use crate::controller::milp_planner::{
        AssetKind, AssetMilpContext, AssetMilpParams, MilpLoadMode,
    };
    use good_lp::{variable, variables};

    fn empty_pool(vars: &mut good_lp::ProblemVariables, n: usize) -> MilpVarPool {
        let grid = GridMilpVars {
            p_imp: (0..n).map(|_| vars.add(variable().min(0.0))).collect(),
            p_exp: (0..n).map(|_| vars.add(variable().min(0.0))).collect(),
            u_grid: (0..n).map(|_| vars.add(variable().binary())).collect(),
            s_imp_viol: (0..n).map(|_| vars.add(variable().min(0.0))).collect(),
            s_exp_viol: (0..n).map(|_| vars.add(variable().min(0.0))).collect(),
        };
        MilpVarPool {
            grid,
            bat: None,
            ev: None,
            heater: None,
            shiftable: vec![],
        }
    }

    fn make_must_run(n: usize) -> EvMilpContext {
        EvMilpContext {
            mode: EvMilpMode::MustRun,
            a_ev: vec![true; n],
            t_dead_step: Some(n - 1),
            p_max_kw: 7.2,
            p_min_kw: 0.0,
            e_core_kwh: 10.0,
            e_extra_max_kwh: 5.0,
            v_extra_eur_kwh: 0.05,
            v_core_eur: 0.0,
            asap_lateness_eur_kwh_h: 0.0,
            free_only: false,
            p_free_cap_kw: None,
            reward_per_slot: false,
            free_early_bias: false,
            budget_eur: None,
            c_imp_eur_kwh: None,
        }
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
            a_ev: vec![true; 4],
            t_dead_step: None,
            p_max_kw: 7.2,
            p_min_kw: 0.0,
            e_core_kwh: 0.0,
            e_extra_max_kwh: 5.0,
            v_extra_eur_kwh: 0.05,
            v_core_eur: 0.0,
            asap_lateness_eur_kwh_h: 0.0,
            free_only: false,
            p_free_cap_kw: None,
            reward_per_slot: false,
            free_early_bias: false,
            budget_eur: None,
            c_imp_eur_kwh: None,
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
            a_ev: vec![false; 4],
            t_dead_step: None,
            p_max_kw: 7.2,
            p_min_kw: 0.0,
            e_core_kwh: 0.0,
            e_extra_max_kwh: 5.0,
            v_extra_eur_kwh: 0.05,
            v_core_eur: 0.0,
            asap_lateness_eur_kwh_h: 0.0,
            free_only: false,
            p_free_cap_kw: None,
            reward_per_slot: false,
            free_early_bias: false,
            budget_eur: None,
            c_imp_eur_kwh: None,
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
            a_ev: a_ev.clone(),
            t_dead_step: None,
            p_max_kw: 7.2,
            p_min_kw: 0.0,
            e_core_kwh: 0.0,
            e_extra_max_kwh: 5.0,
            v_extra_eur_kwh: 0.05,
            v_core_eur: 0.0,
            asap_lateness_eur_kwh_h: 0.0,
            free_only: false,
            p_free_cap_kw: None,
            reward_per_slot: false,
            free_early_bias: false,
            budget_eur: None,
            c_imp_eur_kwh: None,
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
}
