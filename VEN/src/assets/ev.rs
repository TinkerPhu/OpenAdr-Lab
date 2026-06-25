use chrono::{DateTime, Duration, Utc};
use good_lp::{constraint, variable, Constraint, Expression, ProblemVariables, Solution};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{Asset, AssetCapability, AssetState, ControlDescriptor, ControlKind};
use crate::common::{Interpolation, TimeSeries};
use crate::controller::milp_planner::asset_port::{
    EvMilpContext, EvMilpMode, EvMilpVars, EvSolOutput,
};
use crate::entities::asset_params::EvParams;

/// EV Charger config. Positive = charge (import), negative = V2G discharge (export).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvCharger {
    pub max_charge_kw: f64,
    pub max_discharge_kw: f64,
    pub battery_kwh: f64,
    /// Active SOC ceiling — charging stops at this level (BMS limit). Overridable at runtime.
    pub soc_target: f64,
    /// Original profile value — used for snap-back when inject override is released.
    pub soc_target_profile: f64,
    pub default_charge_kw: f64,
    /// V2G floor; 0.0 if not specified in profile.
    pub min_soc: f64,
}

/// EV mutable state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvState {
    /// State of charge in [0.0, 1.0].
    pub soc: f64,
    pub plugged: bool,
    /// Actual power last tick. Positive = charging (import). Negative = V2G (export).
    pub actual_power_kw: f64,
}

impl EvCharger {
    pub fn from_params(cfg: &EvParams) -> Self {
        Self {
            max_charge_kw: cfg.max_charge_kw,
            max_discharge_kw: cfg.max_discharge_kw,
            battery_kwh: cfg.battery_kwh,
            soc_target: cfg.soc_target,
            soc_target_profile: cfg.soc_target,
            default_charge_kw: cfg.default_charge_kw,
            min_soc: 0.0,
        }
    }

    pub fn initial_state(cfg: &EvParams) -> EvState {
        EvState {
            soc: cfg.initial_soc,
            plugged: true,
            actual_power_kw: cfg.max_charge_kw,
        }
    }

    /// Pure physics step. Returns (new_state, actual_power_kw).
    pub fn step_inner(&self, state: &EvState, setpoint_kw: f64, dt: Duration) -> (EvState, f64) {
        if !state.plugged {
            return (
                EvState {
                    actual_power_kw: 0.0,
                    ..state.clone()
                },
                0.0,
            );
        }
        let kw = setpoint_kw.clamp(-self.max_discharge_kw, self.max_charge_kw);
        let kw = if (kw > 0.0 && state.soc >= self.soc_target)
            || (kw < 0.0 && state.soc <= self.min_soc)
        {
            0.0
        } else {
            kw
        };
        let dt_h = dt.num_milliseconds() as f64 / 3_600_000.0;
        let new_soc = (state.soc + (kw * dt_h) / self.battery_kwh).clamp(0.0, 1.0);
        (
            EvState {
                soc: new_soc,
                plugged: state.plugged,
                actual_power_kw: kw,
            },
            kw,
        )
    }

    /// Point-in-time feasible power range.
    pub fn capability_inner(&self, state: &EvState) -> AssetCapability {
        if !state.plugged {
            return AssetCapability {
                max_export_kw: 0.0,
                max_import_kw: 0.0,
            };
        }
        AssetCapability {
            max_export_kw: if state.soc <= self.min_soc {
                0.0
            } else {
                -self.max_discharge_kw
            },
            max_import_kw: if state.soc >= self.soc_target {
                0.0
            } else {
                self.max_charge_kw
            },
        }
    }

    pub fn default_setpoint(&self) -> f64 {
        self.default_charge_kw
    }

    pub fn state_values(&self, state: &EvState) -> HashMap<String, f64> {
        let mut m = HashMap::new();
        m.insert("soc".into(), state.soc);
        m.insert("plugged".into(), if state.plugged { 1.0 } else { 0.0 });
        m.insert("max_charge_kw".into(), self.max_charge_kw);
        m.insert("soc_target".into(), self.soc_target);
        m.insert("battery_kwh".into(), self.battery_kwh);
        m
    }

    /// Compute EV SoC trajectory from MILP charge-power schedule.
    ///
    /// Returns a `Vec<f64>` of length `n + 1` where index `t` is the SoC at the
    /// **start** of slot `t` (index `n` is the SoC at the end of the last slot).
    /// `p_ev_kw[t]` is net charge power (kW) during slot `t`, `dt_h` is slot
    /// duration in hours.  Values are clamped to `[0.0, 1.0]`.
    pub fn soc_trajectory(p_ev_kw: &[f64], soc_init: f64, battery_kwh: f64, dt_h: f64) -> Vec<f64> {
        let n = p_ev_kw.len();
        let mut traj = Vec::with_capacity(n + 1);
        traj.push(soc_init.clamp(0.0, 1.0));
        for t in 0..n {
            let next = traj[t] + p_ev_kw[t] * dt_h / battery_kwh;
            traj.push(next.clamp(0.0, 1.0));
        }
        traj
    }

    /// State values for a future MILP time slot given the SoC at the start of
    /// that slot. Returns `{"soc": <0..1>}`.
    pub fn future_state_values_at(soc: f64) -> HashMap<String, f64> {
        HashMap::from([("soc".into(), soc.clamp(0.0, 1.0))])
    }

    pub fn control_schema(&self) -> Vec<ControlDescriptor> {
        vec![
            ControlDescriptor {
                key: "ev_plugged".into(),
                label: "Plugged In".into(),
                kind: ControlKind::Switch,
                min: None,
                max: None,
                unit: "".into(),
                display_scale: None,
            },
            ControlDescriptor {
                key: "ev_soc_target".into(),
                label: "Charge Target".into(),
                kind: ControlKind::Slider,
                min: Some(0.1),
                max: Some(1.0),
                unit: "%".into(),
                display_scale: Some(100.0),
            },
        ]
    }

    pub fn reset(&self, state: &mut EvState, values: HashMap<String, f64>) {
        if let Some(&soc) = values.get("soc") {
            state.soc = soc.clamp(0.0, 1.0);
        }
    }

    pub fn update_config(&mut self, values: HashMap<String, f64>) {
        if let Some(&v) = values.get("max_charge_kw") {
            self.max_charge_kw = v.max(0.0);
        }
    }

    pub fn forecast(&self, state: &EvState, timespan: Duration) -> TimeSeries {
        if timespan <= Duration::zero() {
            return TimeSeries::empty(Interpolation::Step);
        }
        let now = Utc::now();
        let power = if state.plugged {
            state.actual_power_kw
        } else {
            0.0
        };
        TimeSeries {
            samples: vec![(now, power), (now + timespan, power)],
            interpolation: Interpolation::Step,
        }
    }

    pub fn default_comfort_rates(&self) -> Vec<crate::entities::asset::ComfortRate> {
        vec![
            crate::entities::asset::ComfortRate {
                fill: 0.0,
                max_marginal_price: 0.35,
                max_marginal_co2: 0.0,
            },
            crate::entities::asset::ComfortRate {
                fill: 1.0,
                max_marginal_price: 0.05,
                max_marginal_co2: 0.0,
            },
        ]
    }

    pub fn default_completion_policy(&self) -> crate::entities::asset::CompletionPolicy {
        crate::entities::asset::CompletionPolicy::Stop
    }

    pub fn default_post_deadline_comfort_bid(&self) -> Option<f64> {
        None
    }

    pub fn resolve_request_target(
        &self,
        state: &EvState,
        target_soc: Option<f64>,
        desired_power_kw: Option<f64>,
    ) -> Option<(f64, f64)> {
        let target = target_soc.unwrap_or(self.soc_target);
        let delta = (target - state.soc).max(0.0);
        let kwh = delta * self.battery_kwh;
        if kwh < 1e-6 {
            return None;
        }
        Some((kwh, desired_power_kw.unwrap_or(self.max_charge_kw)))
    }
}

impl Asset for EvCharger {
    fn step(&self, state: &AssetState, setpoint_kw: f64, dt: Duration) -> (AssetState, f64) {
        let AssetState::Ev(s) = state else {
            unreachable!("EvCharger/state mismatch")
        };
        let (ns, p) = self.step_inner(s, setpoint_kw, dt);
        (AssetState::Ev(ns), p)
    }

    fn capability(&self, state: &AssetState) -> AssetCapability {
        let AssetState::Ev(s) = state else {
            unreachable!()
        };
        self.capability_inner(s)
    }
}

// ── EV MILP plugin types ──────────────────────────────────────────────────────
// Struct/enum definitions live in `controller::milp_planner::asset_port`.
// Method implementations below are cross-file inherent impl blocks — valid Rust.

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
        for t in 0..n {
            if t <= t_dlim {
                expr += dt_h[t] * v.p_ev[t];
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
    pub fn objective(
        &self,
        v: &EvMilpVars,
        startup_eur: f64,
        ramp_eur_kw: f64,
        w_services: f64,
        n: usize,
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
        if self.mode != EvMilpMode::MustNotRun {
            obj += -(w_services * self.v_extra_eur_kwh) * v.e_ev_extra;
        }
        if self.mode == EvMilpMode::MayRun {
            obj += -(w_services * self.v_core_eur) * v.z_ev_core;
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
    #[allow(clippy::too_many_arguments)]
    pub fn from_state(
        state: &super::AssetState,
        cfg: &EvCharger,
        n: usize,
        step_s: u64,
        now: DateTime<Utc>,
        ev_session: Option<&crate::entities::device_session::EvSession>,
        min_charge_kw: f64,
        v_ev_extra_eur_kwh: f64,
        v_ev_core_eur_kwh: f64,
    ) -> Self {
        let plugged = if let super::AssetState::Ev(s) = state {
            s.plugged
        } else {
            false
        };
        if !plugged {
            return Self {
                mode: EvMilpMode::MustNotRun,
                a_ev: vec![false; n],
                t_dead_step: None,
                p_max_kw: cfg.max_charge_kw,
                p_min_kw: min_charge_kw,
                e_core_kwh: 0.0,
                e_extra_max_kwh: cfg.battery_kwh * (1.0 - cfg.soc_target),
                v_extra_eur_kwh: v_ev_extra_eur_kwh,
                v_core_eur: 0.0,
            };
        }
        if let Some(session) = ev_session {
            let current_soc = if let super::AssetState::Ev(s) = state {
                s.soc
            } else {
                0.0
            };
            let core_kwh = ((session.target_soc - current_soc) * cfg.battery_kwh).max(0.0);
            let mode = if session.soft_deadline {
                EvMilpMode::MayRun
            } else {
                EvMilpMode::MustRun
            };
            let secs = (session.departure_time - now).num_seconds();
            let t_dead = (secs / step_s as i64).clamp(0, (n.saturating_sub(1)) as i64) as usize;
            Self {
                mode,
                a_ev: (0..n).map(|t| t <= t_dead).collect(),
                t_dead_step: Some(t_dead),
                p_max_kw: cfg.max_charge_kw,
                p_min_kw: min_charge_kw,
                e_core_kwh: core_kwh,
                e_extra_max_kwh: cfg.battery_kwh * (1.0 - session.target_soc),
                v_extra_eur_kwh: v_ev_extra_eur_kwh,
                v_core_eur: if session.soft_deadline {
                    core_kwh * v_ev_core_eur_kwh
                } else {
                    0.0
                },
            }
        } else {
            // Plugged, no session: slots available but no charging obligation
            Self {
                mode: EvMilpMode::MustNotRun,
                a_ev: vec![true; n],
                t_dead_step: None,
                p_max_kw: cfg.max_charge_kw,
                p_min_kw: min_charge_kw,
                e_core_kwh: 0.0,
                e_extra_max_kwh: cfg.battery_kwh * (1.0 - cfg.soc_target),
                v_extra_eur_kwh: v_ev_extra_eur_kwh,
                v_core_eur: 0.0,
            }
        }
    }
}

impl crate::controller::milp_planner::AssetMilpContext for EvMilpContext {
    fn asset_id(&self) -> &str {
        "ev"
    }

    fn asset_kind(&self) -> crate::controller::milp_planner::AssetKind {
        crate::controller::milp_planner::AssetKind::Ev
    }

    fn milp_params(
        &self,
        _n: usize,
        _step_s: u64,
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
        _dt_h: &[f64],
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
        )
    }
}

impl EvCharger {
    /// Declare all LP variables for this EV charger into `vars`. Delegates to `EvMilpContext::declare_vars`.
    pub fn declare_milp_vars(
        &self,
        ctx: &EvMilpContext,
        n: usize,
        c_startup_eur: f64,
        c_ramp_eur_kw: f64,
        vars: &mut ProblemVariables,
    ) -> EvMilpVars {
        ctx.declare_vars(n, c_startup_eur, c_ramp_eur_kw, vars)
    }

    /// Build the energy accumulator expression up to the deadline step. Delegates to `EvMilpContext::energy_expr`.
    pub fn milp_energy_expr(
        &self,
        ctx: &EvMilpContext,
        v: &EvMilpVars,
        n: usize,
        dt_h: &[f64],
    ) -> Expression {
        ctx.energy_expr(v, n, dt_h)
    }

    /// Generate all MILP constraints for this EV charger. Delegates to `EvMilpContext::constraints`.
    pub fn milp_constraints(
        &self,
        ctx: &EvMilpContext,
        v: &EvMilpVars,
        n: usize,
        dt_h: &[f64],
    ) -> Vec<Constraint> {
        ctx.constraints(v, n, dt_h)
    }

    /// EV objective contribution. Delegates to `EvMilpContext::objective`.
    pub fn milp_objective(
        &self,
        ctx: &EvMilpContext,
        v: &EvMilpVars,
        startup_eur: f64,
        ramp_eur_kw: f64,
        w_services: f64,
        n: usize,
    ) -> Expression {
        ctx.objective(v, startup_eur, ramp_eur_kw, w_services, n)
    }

    /// Read back the EV solution from the solved model. Delegates to `EvMilpContext::read_solution`.
    pub fn read_milp_solution(&self, sol: &impl Solution, v: &EvMilpVars, n: usize) -> EvSolOutput {
        EvMilpContext::read_solution(sol, v, n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ev(plugged: bool, soc: f64, actual_power_kw: f64) -> (EvCharger, EvState) {
        let cfg = EvCharger {
            max_charge_kw: 7.4,
            max_discharge_kw: 0.0,
            battery_kwh: 40.0,
            soc_target: 0.8,
            soc_target_profile: 0.8,
            default_charge_kw: 7.4,
            min_soc: 0.0,
        };
        let state = EvState {
            soc,
            plugged,
            actual_power_kw,
        };
        (cfg, state)
    }

    #[test]
    fn forecast_not_plugged_returns_zero() {
        let (ev, state) = make_ev(false, 0.5, 7.4);
        let series = ev.forecast(&state, Duration::seconds(3600));
        for (_, v) in &series.samples {
            assert_eq!(*v, 0.0, "Unplugged EV must return zero power");
        }
    }

    #[test]
    fn forecast_zero_timespan_returns_empty() {
        let (ev, state) = make_ev(true, 0.5, 7.4);
        let series = ev.forecast(&state, Duration::zero());
        assert!(series.samples.is_empty());
    }

    #[test]
    fn forecast_has_two_samples_with_boundary() {
        let (ev, state) = make_ev(true, 0.5, 7.4);
        let timespan = Duration::seconds(600);
        let before = Utc::now();
        let series = ev.forecast(&state, timespan);
        let after = Utc::now();
        assert_eq!(
            series.samples.len(),
            2,
            "Step forecast must have exactly 2 samples"
        );
        let last_ts = series.samples.last().unwrap().0;
        assert!(last_ts >= before + timespan && last_ts <= after + timespan);
    }

    #[test]
    fn ev_charges_and_stops_at_full() {
        let (ev, mut state) = make_ev(true, 0.99, 0.0);
        let ev = EvCharger {
            max_charge_kw: 10.0,
            soc_target: 1.0,
            soc_target_profile: 1.0,
            battery_kwh: 10.0,
            ..ev
        };
        for _ in 0..1000 {
            let (ns, _) = ev.step_inner(&state, 10.0, Duration::seconds(1));
            state = ns;
        }
        assert!((state.soc - 1.0).abs() < 0.001);
        let (_, actual) = ev.step_inner(&state, 10.0, Duration::seconds(1));
        assert_eq!(actual, 0.0);
    }

    #[test]
    fn ev_stops_charging_at_soc_target() {
        // soc_target = 0.8 — charging must stop there, not at 1.0
        let (ev, mut state) = make_ev(true, 0.0, 0.0);
        for _ in 0..100_000 {
            let (ns, _) = ev.step_inner(&state, 7.4, Duration::seconds(1));
            state = ns;
        }
        assert!(
            state.soc <= 0.8 + 0.01,
            "soc should not exceed soc_target (0.8), got {}",
            state.soc
        );
        let (_, actual) = ev.step_inner(&state, 7.4, Duration::seconds(1));
        assert_eq!(actual, 0.0, "charging must stop at soc_target");
    }

    #[test]
    fn ev_discharges_v2g_and_stops_at_empty() {
        let ev = EvCharger {
            max_charge_kw: 10.0,
            max_discharge_kw: 10.0,
            battery_kwh: 10.0,
            soc_target: 1.0,
            soc_target_profile: 1.0,
            default_charge_kw: 0.0,
            min_soc: 0.0,
        };
        let mut state = EvState {
            soc: 0.01,
            plugged: true,
            actual_power_kw: 0.0,
        };
        for _ in 0..1000 {
            let (ns, _) = ev.step_inner(&state, -10.0, Duration::seconds(1));
            state = ns;
        }
        assert!((state.soc - 0.0).abs() < 0.001);
        let (_, actual) = ev.step_inner(&state, -10.0, Duration::seconds(1));
        assert_eq!(actual, 0.0);
    }

    #[test]
    fn ev_capability_zero_at_soc_target() {
        // When soc == soc_target the BMS ceiling is hit: capability must report
        // max_import_kw = 0 so the planner fires SocCeiling, not a phantom allocation.
        let (ev, state) = make_ev(true, 0.8, 0.0); // soc_target = 0.8 in make_ev
        let cap = ev.capability_inner(&state);
        assert_eq!(
            cap.max_import_kw, 0.0,
            "capability must be 0 when soc equals soc_target"
        );
    }

    #[test]
    fn ev_capability_positive_below_soc_target() {
        let (ev, state) = make_ev(true, 0.5, 0.0); // soc 0.5 < soc_target 0.8
        let cap = ev.capability_inner(&state);
        assert!(
            cap.max_import_kw > 0.0,
            "capability must be positive when soc < soc_target"
        );
        assert!((cap.max_import_kw - ev.max_charge_kw).abs() < 1e-9);
    }

    // T012: EvCharger::soc_trajectory and future_state_values_at.
    #[test]
    fn soc_trajectory_charges_monotonically() {
        // 5 slots of 1 kW charging, 10 kWh battery, dt_h = 1h → each slot +0.1 SoC
        let p_ev = vec![1.0_f64; 5];
        let traj = EvCharger::soc_trajectory(&p_ev, 0.0, 10.0, 1.0);
        assert_eq!(traj.len(), 6);
        for i in 1..=5 {
            assert!(traj[i] > traj[i - 1], "SoC must increase during charging");
        }
        assert!(
            (traj[5] - 0.5).abs() < 1e-9,
            "expected final soc=0.5, got {}",
            traj[5]
        );
    }

    #[test]
    fn soc_trajectory_clamps_at_one() {
        // Over-charge scenario: 1000 slots of 10 kW charging
        let p_ev = vec![10.0_f64; 1000];
        let traj = EvCharger::soc_trajectory(&p_ev, 0.5, 10.0, 1.0);
        assert_eq!(*traj.last().unwrap(), 1.0);
    }

    #[test]
    fn future_state_values_at_returns_soc() {
        let vals = EvCharger::future_state_values_at(0.65);
        let soc = vals["soc"];
        assert!((soc - 0.65).abs() < 1e-9, "expected soc=0.65, got {soc}");
    }

    #[test]
    fn future_state_values_at_clamps() {
        assert_eq!(EvCharger::future_state_values_at(-0.1)["soc"], 0.0);
        assert_eq!(EvCharger::future_state_values_at(1.5)["soc"], 1.0);
    }
}

#[cfg(test)]
mod param_tests {
    use super::*;

    #[test]
    fn ev_params_default_values() {
        assert!((EvParams::default().max_charge_kw - 7.4).abs() < f64::EPSILON);
    }

    #[test]
    fn ev_params_min_charge_enforced() {
        let params = EvParams {
            min_charge_kw: 2.0,
            ..EvParams::default()
        };
        assert!((params.min_charge_kw - 2.0).abs() < f64::EPSILON);
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
        match ctx.milp_params(4, 300, chrono::Utc::now()) {
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
        };
        match ctx.milp_params(4, 300, chrono::Utc::now()) {
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
        };
        match ctx.milp_params(4, 300, chrono::Utc::now()) {
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
        };
        match ctx.milp_params(n, 300, chrono::Utc::now()) {
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
