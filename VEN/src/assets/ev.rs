use chrono::{Duration, Utc};
use good_lp::{constraint, variable, Constraint, Expression, ProblemVariables, Solution, Variable};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{
    Asset, AssetCapabilities, AssetCapability, AssetState, ControlDescriptor, ControlKind,
    EnergyState,
};
use crate::common::{Interpolation, TimeSeries};

use crate::profile::EvConfig;

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
    pub fn from_config(cfg: &EvConfig) -> Self {
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

    pub fn initial_state(cfg: &EvConfig) -> EvState {
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
        let kw = if kw > 0.0 && state.soc >= self.soc_target {
            0.0
        } else if kw < 0.0 && state.soc <= self.min_soc {
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

    pub fn capabilities(&self, asset_id: &str, state: &EvState) -> AssetCapabilities {
        AssetCapabilities {
            asset_id: asset_id.to_string(),
            max_import_kw: self.max_charge_kw,
            max_export_kw: self.max_discharge_kw,
            is_flexible: true,
            energy_state: Some(EnergyState {
                current_kwh: state.soc * self.battery_kwh,
                min_kwh: 0.0,
                max_kwh: self.battery_kwh,
            }),
            availability: None,
        }
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

/// Scheduling mode for the EV in the MILP model.
#[derive(Debug, Clone, PartialEq)]
pub enum EvMilpMode {
    /// Hard energy requirement — must be met within the deadline.
    MustRun,
    /// Soft energy target — controlled by a reward term in the objective.
    MayRun,
    /// EV absent, unplugged, or no charging session — power fixed to zero.
    MustNotRun,
}

/// Pre-computed MILP parameters for one EV charger and planning cycle.
#[derive(Debug, Clone)]
pub struct EvMilpContext {
    pub mode: EvMilpMode,
    /// Per-step availability mask (false forces p_ev[t] = 0).
    pub a_ev: Vec<bool>,
    /// Last step index that counts toward the core energy sum (None = open horizon).
    pub t_dead_step: Option<usize>,
    /// Maximum charge power [kW].
    pub p_max_kw: f64,
    /// Semi-continuous minimum charge power [kW] (prevents trickle charging).
    pub p_min_kw: f64,
    /// Core energy requirement [kWh] from the active session.
    pub e_core_kwh: f64,
    /// Opportunistic headroom = battery_kwh × (1 − soc_target) [kWh].
    pub e_extra_max_kwh: f64,
    /// Reward per kWh of extra opportunistic charging above core [€/kWh].
    pub v_extra_eur_kwh: f64,
}

/// Typed LP variable handles for one EV charger in the MILP model.
#[derive(Debug, Clone)]
pub struct EvMilpVars {
    pub p_ev: Vec<Variable>,
    /// Binary on/off flag per slot (respects availability mask).
    pub z_ev_on: Vec<Variable>,
    /// Binary: 1 when EV core target is met (MayRun only; fixed 0 otherwise).
    pub z_ev_core: Variable,
    /// Total extra energy above core requirement [kWh].
    pub e_ev_extra: Variable,
    /// Startup transition binaries (empty when startup penalty disabled).
    pub delta_ev: Vec<Variable>,
    /// Ramp variables |p_ev[t] − p_ev[t−1]| (empty when ramp penalty disabled).
    pub delta_ev_ramp: Vec<Variable>,
    /// Semi-continuous minimum charge power [kW] — cached for cross-asset interactions.
    pub p_min_kw: f64,
}

/// Per-EV MILP solution readback.
#[derive(Debug, Clone)]
pub struct EvSolOutput {
    pub p_ev_kw: Vec<f64>,
    pub z_ev_on: Vec<f64>,
    pub e_ev_extra_kwh: f64,
    pub z_ev_core: f64,
}

impl EvCharger {
    /// Declare all LP variables for this EV charger into `vars`.
    pub fn declare_milp_vars(
        &self,
        ctx: &EvMilpContext,
        n: usize,
        c_startup_eur: f64,
        c_ramp_eur_kw: f64,
        vars: &mut ProblemVariables,
    ) -> EvMilpVars {
        let p_ev = (0..n)
            .map(|_| {
                if ctx.mode == EvMilpMode::MustNotRun {
                    vars.add(variable().min(0.0).max(0.0))
                } else {
                    vars.add(variable().min(0.0).max(ctx.p_max_kw))
                }
            })
            .collect();
        let z_ev_on = (0..n)
            .map(|t| {
                if ctx.mode == EvMilpMode::MustNotRun {
                    vars.add(variable().min(0.0).max(0.0))
                } else {
                    let ub = if ctx.a_ev[t] { 1.0 } else { 0.0 };
                    vars.add(variable().max(ub).binary())
                }
            })
            .collect();
        let z_ev_core = if ctx.mode == EvMilpMode::MayRun {
            vars.add(variable().binary())
        } else {
            vars.add(variable().min(0.0).max(0.0))
        };
        let e_ev_extra = if ctx.mode == EvMilpMode::MustNotRun {
            vars.add(variable().min(0.0).max(0.0))
        } else {
            vars.add(variable().min(0.0).max(ctx.e_extra_max_kwh))
        };
        let delta_ev = if ctx.mode != EvMilpMode::MustNotRun && n > 1 && c_startup_eur > 0.0 {
            (0..n - 1).map(|_| vars.add(variable().binary())).collect()
        } else {
            vec![]
        };
        let delta_ev_ramp = if ctx.mode != EvMilpMode::MustNotRun && n > 1 && c_ramp_eur_kw > 0.0 {
            (0..n - 1).map(|_| vars.add(variable().min(0.0))).collect()
        } else {
            vec![]
        };
        EvMilpVars { p_ev, z_ev_on, z_ev_core, e_ev_extra, delta_ev, delta_ev_ramp, p_min_kw: ctx.p_min_kw }
    }

    /// Build the energy accumulator expression up to the deadline step.
    pub fn milp_energy_expr(
        &self,
        ctx: &EvMilpContext,
        v: &EvMilpVars,
        n: usize,
        dt_h: f64,
    ) -> Expression {
        let t_dlim = ctx.t_dead_step.unwrap_or(n.saturating_sub(1));
        let mut expr = Expression::from(0.0);
        for t in 0..n {
            if t <= t_dlim {
                expr += dt_h * v.p_ev[t];
            }
        }
        expr
    }

    /// Generate all MILP constraints for this EV charger.
    pub fn milp_constraints(
        &self,
        ctx: &EvMilpContext,
        v: &EvMilpVars,
        n: usize,
        dt_h: f64,
    ) -> Vec<Constraint> {
        let mut cs: Vec<Constraint> = Vec::new();
        let ev_energy = self.milp_energy_expr(ctx, v, n, dt_h);

        for t in 0..n {
            if ctx.mode != EvMilpMode::MustNotRun {
                let ev_ub = if ctx.a_ev[t] { ctx.p_max_kw } else { 0.0 };
                cs.push(constraint!(v.p_ev[t] >= ctx.p_min_kw * v.z_ev_on[t]));
                cs.push(constraint!(v.p_ev[t] <= ev_ub * v.z_ev_on[t]));
            }
        }
        match ctx.mode {
            EvMilpMode::MustRun => {
                cs.push(constraint!(ev_energy.clone() >= ctx.e_core_kwh));
                cs.push(constraint!(ev_energy <= ctx.e_core_kwh + v.e_ev_extra));
            }
            EvMilpMode::MayRun => {
                cs.push(constraint!(ev_energy.clone() >= ctx.e_core_kwh * v.z_ev_core));
                cs.push(constraint!(
                    ev_energy <= ctx.e_core_kwh * v.z_ev_core + v.e_ev_extra
                ));
                cs.push(constraint!(v.e_ev_extra <= ctx.e_extra_max_kwh * v.z_ev_core));
            }
            EvMilpMode::MustNotRun => {}
        }
        for i in 0..v.delta_ev.len() {
            let t = i + 1;
            cs.push(constraint!(v.delta_ev[i] >= v.z_ev_on[t] - v.z_ev_on[t - 1]));
        }
        for i in 0..v.delta_ev_ramp.len() {
            let t = i + 1;
            cs.push(constraint!(v.delta_ev_ramp[i] >= v.p_ev[t] - v.p_ev[t - 1]));
            cs.push(constraint!(v.delta_ev_ramp[i] >= v.p_ev[t - 1] - v.p_ev[t]));
        }
        cs
    }

    /// EV objective contribution: startup penalty + ramp penalty + extra-charge reward.
    /// The reward sign is negative (subtracted from the minimised objective).
    pub fn milp_objective(
        &self,
        ctx: &EvMilpContext,
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
        if ctx.mode != EvMilpMode::MustNotRun {
            obj += -(w_services * ctx.v_extra_eur_kwh) * v.e_ev_extra;
        }
        obj
    }

    /// Read back the EV solution from the solved model.
    pub fn read_milp_solution(
        &self,
        sol: &impl Solution,
        v: &EvMilpVars,
        n: usize,
    ) -> EvSolOutput {
        EvSolOutput {
            p_ev_kw: (0..n).map(|t| sol.value(v.p_ev[t])).collect(),
            z_ev_on: (0..n).map(|t| sol.value(v.z_ev_on[t])).collect(),
            e_ev_extra_kwh: sol.value(v.e_ev_extra),
            z_ev_core: sol.value(v.z_ev_core),
        }
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
        assert_eq!(cap.max_import_kw, 0.0, "capability must be 0 when soc equals soc_target");
    }

    #[test]
    fn ev_capability_positive_below_soc_target() {
        let (ev, state) = make_ev(true, 0.5, 0.0); // soc 0.5 < soc_target 0.8
        let cap = ev.capability_inner(&state);
        assert!(cap.max_import_kw > 0.0, "capability must be positive when soc < soc_target");
        assert!((cap.max_import_kw - ev.max_charge_kw).abs() < 1e-9);
    }
}
