use chrono::{DateTime, Duration, Utc};
use good_lp::{constraint, variable, Constraint, Expression, ProblemVariables, Solution, Variable};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{
    Asset, AssetCapabilities, AssetCapability, AssetState, ControlDescriptor,
    EnergyState,
};
use crate::common::{Interpolation, TimeSeries};
use crate::profile::BatteryConfig;

/// Battery storage config. Bidirectional.
/// Positive setpoint = charge (import), negative = discharge (export).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Battery {
    pub capacity_kwh: f64,
    pub max_charge_kw: f64,
    pub max_discharge_kw: f64,
    pub round_trip_efficiency: f64,
    pub min_soc: f64,
}

/// Battery mutable state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatteryState {
    /// State of charge in [0.0, 1.0]. 0.0 = empty, 1.0 = full.
    pub soc: f64,
    /// Actual power last tick. Positive = charging (import). Negative = discharging (export).
    pub actual_power_kw: f64,
}

impl Battery {
    pub fn from_config(cfg: &BatteryConfig) -> Self {
        Self {
            capacity_kwh: cfg.capacity_kwh,
            max_charge_kw: cfg.max_charge_kw,
            max_discharge_kw: cfg.max_discharge_kw,
            round_trip_efficiency: cfg.round_trip_efficiency,
            min_soc: cfg.min_soc,
        }
    }

    pub fn initial_state(cfg: &BatteryConfig) -> BatteryState {
        BatteryState {
            soc: cfg.initial_soc,
            actual_power_kw: 0.0,
        }
    }

    /// Pure physics step. Returns (new_state, actual_power_kw).
    pub fn step_inner(
        &self,
        state: &BatteryState,
        setpoint_kw: f64,
        dt: Duration,
    ) -> (BatteryState, f64) {
        let dt_h = dt.num_milliseconds() as f64 / 3_600_000.0;
        let clamped = setpoint_kw
            .max(-self.max_discharge_kw)
            .min(self.max_charge_kw);
        let actual = if clamped > 0.0 && state.soc >= 1.0 {
            0.0
        } else if clamped < 0.0 && state.soc <= self.min_soc {
            0.0
        } else {
            clamped
        };
        let energy_kwh = actual
            * dt_h
            * if actual > 0.0 {
                self.round_trip_efficiency
            } else {
                1.0
            };
        let new_soc = (state.soc + energy_kwh / self.capacity_kwh).clamp(0.0, 1.0);
        (
            BatteryState {
                soc: new_soc,
                actual_power_kw: actual,
            },
            actual,
        )
    }

    /// Point-in-time feasible power range.
    pub fn capability_inner(&self, state: &BatteryState) -> AssetCapability {
        AssetCapability {
            max_export_kw: if state.soc <= self.min_soc {
                0.0
            } else {
                -self.max_discharge_kw
            },
            max_import_kw: if state.soc >= 1.0 {
                0.0
            } else {
                self.max_charge_kw
            },
        }
    }

    pub fn default_setpoint(&self) -> f64 {
        0.0 // hold by default; dispatcher controls
    }

    pub fn state_values(&self, state: &BatteryState) -> HashMap<String, f64> {
        let mut m = HashMap::new();
        m.insert("soc".into(), state.soc);
        m.insert("capacity_kwh".into(), self.capacity_kwh);
        m.insert("max_charge_kw".into(), self.max_charge_kw);
        m.insert("max_discharge_kw".into(), self.max_discharge_kw);
        m.insert("min_soc".into(), self.min_soc);
        m
    }

    pub fn capabilities(&self, asset_id: &str, state: &BatteryState) -> AssetCapabilities {
        let cap = self.capability_inner(state);
        AssetCapabilities {
            asset_id: asset_id.to_string(),
            max_import_kw: cap.max_import_kw,
            max_export_kw: self.max_discharge_kw,
            is_flexible: true,
            energy_state: Some(EnergyState {
                current_kwh: state.soc * self.capacity_kwh,
                min_kwh: self.min_soc * self.capacity_kwh,
                max_kwh: self.capacity_kwh,
            }),
            availability: None,
        }
    }

    pub fn control_schema(&self) -> Vec<ControlDescriptor> {
        vec![]
    }

    pub fn reset(&self, state: &mut BatteryState, values: HashMap<String, f64>) {
        if let Some(&soc) = values.get("soc") {
            state.soc = soc.clamp(0.0, 1.0);
        }
    }

    pub fn update_config(&mut self, values: HashMap<String, f64>) {
        if let Some(&v) = values.get("capacity_kwh") {
            self.capacity_kwh = v.max(0.1);
        }
        if let Some(&v) = values.get("min_soc") {
            self.min_soc = v.clamp(0.0, 1.0);
        }
    }

    pub fn forecast(&self, state: &BatteryState, timespan: Duration) -> TimeSeries {
        if timespan <= Duration::zero() {
            return TimeSeries::empty(Interpolation::Linear);
        }
        let now = Utc::now();
        let end = now + timespan;
        let setpoint = state
            .actual_power_kw
            .clamp(-self.max_discharge_kw, self.max_charge_kw);
        let mut samples: Vec<(DateTime<Utc>, f64)> = Vec::new();

        let mut t = now;
        let mut soc = state.soc;

        while t < end {
            let kw = if setpoint > 0.0 && soc >= 1.0 {
                0.0
            } else if setpoint < 0.0 && soc <= self.min_soc {
                0.0
            } else {
                setpoint
            };
            samples.push((t, kw));

            let dt_h = 1.0 / 60.0;
            if kw > 0.0 {
                soc += (kw * dt_h * self.round_trip_efficiency) / self.capacity_kwh;
            } else {
                soc += (kw * dt_h) / self.capacity_kwh;
            }
            soc = soc.clamp(0.0, 1.0);
            t = t + Duration::seconds(60);
        }
        let end_kw = if setpoint > 0.0 && soc >= 1.0 {
            0.0
        } else if setpoint < 0.0 && soc <= self.min_soc {
            0.0
        } else {
            setpoint
        };
        samples.push((end, end_kw));

        TimeSeries {
            samples,
            interpolation: Interpolation::Linear,
        }
    }

    pub fn default_comfort_rates(&self) -> Vec<crate::entities::asset::ComfortRate> {
        vec![
            crate::entities::asset::ComfortRate {
                fill: 0.0,
                max_marginal_price: 0.20,
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
        state: &BatteryState,
        target_soc: Option<f64>,
        desired_power_kw: Option<f64>,
    ) -> Option<(f64, f64)> {
        let target = target_soc.unwrap_or(1.0);
        let delta = (target - state.soc).max(0.0);
        let kwh = delta * self.capacity_kwh;
        if kwh < 1e-6 {
            return None;
        }
        Some((kwh, desired_power_kw.unwrap_or(self.max_charge_kw)))
    }
}

// ── Battery MILP plugin types ─────────────────────────────────────────────────

/// Pre-computed MILP parameters for one battery instance and planning cycle.
/// Built from live state; consumed by `declare_milp_vars` and the constraint/
/// objective methods. Avoids repeated field accesses inside tight solver loops.
#[derive(Debug, Clone)]
pub struct BatteryMilpContext {
    pub e_nom_kwh: f64,
    /// Live SoC × capacity — NOT the profile's initial_soc.
    pub e_init_kwh: f64,
    pub e_min_kwh: f64,
    pub e_max_kwh: f64,
    pub p_ch_max_kw: f64,
    pub p_dis_max_kw: f64,
    /// One-way charge efficiency = √(round_trip_efficiency)
    pub eff_ch: f64,
    /// One-way discharge efficiency = √(round_trip_efficiency)
    pub eff_dis: f64,
}

/// Typed LP variable handles for one battery in the MILP model.
/// `z_active`, `delta_active`, and `delta_ramp` are empty vecs when the
/// corresponding penalty coefficients are zero (feature disabled).
#[derive(Debug, Clone)]
pub struct BatteryMilpVars {
    pub p_ch: Vec<Variable>,
    pub p_dis: Vec<Variable>,
    pub u_bat: Vec<Variable>,
    /// SoC trajectory, len = n + 1 (index 0 = initial SoC, fixed).
    pub e_bat: Vec<Variable>,
    /// Activity indicator per slot (1 = charging or discharging). Empty if startup penalty disabled.
    pub z_active: Vec<Variable>,
    /// Idle→active transition binary per slot boundary. Empty if startup penalty disabled.
    pub delta_active: Vec<Variable>,
    /// |net_bat[t] − net_bat[t−1]| ramp variable. Empty if ramp penalty disabled.
    pub delta_ramp: Vec<Variable>,
    /// Maximum discharge power [kW] — cached from context for cross-asset interactions.
    pub dis_max_kw: f64,
}

/// Per-battery MILP solution readback.
#[derive(Debug, Clone)]
pub struct BatterySolOutput {
    pub p_ch_kw: Vec<f64>,
    pub p_dis_kw: Vec<f64>,
    /// SoC trajectory [kWh], len = n + 1.
    pub e_kwh: Vec<f64>,
}

impl BatteryMilpContext {
    /// Declare all LP variables for this battery. Context-side canonical implementation;
    /// `Battery::declare_milp_vars` delegates here.
    pub fn declare_vars(
        &self,
        n: usize,
        c_startup_eur: f64,
        c_ramp_eur_kw: f64,
        vars: &mut ProblemVariables,
    ) -> BatteryMilpVars {
        let p_ch = (0..n)
            .map(|_| vars.add(variable().min(0.0).max(self.p_ch_max_kw)))
            .collect();
        let p_dis = (0..n)
            .map(|_| vars.add(variable().min(0.0).max(self.p_dis_max_kw)))
            .collect();
        let u_bat = (0..n).map(|_| vars.add(variable().binary())).collect();
        let e_bat = (0..=n)
            .map(|i| {
                if i == 0 {
                    vars.add(variable().min(self.e_init_kwh).max(self.e_init_kwh))
                } else {
                    vars.add(variable().min(self.e_min_kwh).max(self.e_max_kwh))
                }
            })
            .collect();
        let z_active = if n > 1 && c_startup_eur > 0.0 {
            (0..n).map(|_| vars.add(variable().binary())).collect()
        } else {
            vec![]
        };
        let delta_active = if n > 1 && c_startup_eur > 0.0 {
            (0..n - 1).map(|_| vars.add(variable().binary())).collect()
        } else {
            vec![]
        };
        let delta_ramp = if n > 1 && c_ramp_eur_kw > 0.0 {
            (0..n - 1).map(|_| vars.add(variable().min(0.0))).collect()
        } else {
            vec![]
        };
        BatteryMilpVars {
            p_ch,
            p_dis,
            u_bat,
            e_bat,
            z_active,
            delta_active,
            delta_ramp,
            dis_max_kw: self.p_dis_max_kw,
        }
    }

    /// Generate all MILP constraints for this battery. Context-side canonical implementation.
    pub fn constraints(&self, v: &BatteryMilpVars, n: usize, dt_h: f64) -> Vec<Constraint> {
        let mut cs: Vec<Constraint> = Vec::new();
        for t in 0..n {
            cs.push(constraint!(v.p_ch[t] <= self.p_ch_max_kw * v.u_bat[t]));
            cs.push(constraint!(v.p_dis[t] <= self.p_dis_max_kw * (1.0 - v.u_bat[t])));
            cs.push(constraint!(
                v.e_bat[t + 1]
                    == v.e_bat[t]
                        + dt_h * self.eff_ch * v.p_ch[t]
                        - dt_h * (1.0 / self.eff_dis) * v.p_dis[t]
            ));
            if let Some(&z) = v.z_active.get(t) {
                let big_m = self.p_ch_max_kw + self.p_dis_max_kw;
                cs.push(constraint!(v.p_ch[t] + v.p_dis[t] <= big_m * z));
            }
        }
        for i in 0..v.delta_active.len() {
            let t = i + 1;
            cs.push(constraint!(v.delta_active[i] >= v.z_active[t] - v.z_active[t - 1]));
        }
        for i in 0..v.delta_ramp.len() {
            let t = i + 1;
            cs.push(constraint!(
                v.delta_ramp[i]
                    >= (v.p_ch[t] - v.p_dis[t]) - (v.p_ch[t - 1] - v.p_dis[t - 1])
            ));
            cs.push(constraint!(
                v.delta_ramp[i]
                    >= (v.p_ch[t - 1] - v.p_dis[t - 1]) - (v.p_ch[t] - v.p_dis[t])
            ));
        }
        cs.push(constraint!(v.e_bat[n] >= self.e_init_kwh));
        cs
    }

    /// Battery objective contribution. Associated function (no `self` needed — no ctx params used).
    pub fn objective(
        v: &BatteryMilpVars,
        c_wear_eur_kwh: f64,
        c_startup_eur: f64,
        c_ramp_eur_kw: f64,
        n: usize,
        dt_h: f64,
    ) -> Expression {
        let mut obj = Expression::from(0.0);
        for t in 0..n {
            obj += (c_wear_eur_kwh * dt_h) * v.p_ch[t];
            obj += (c_wear_eur_kwh * dt_h) * v.p_dis[t];
            if t >= 1 {
                if let Some(&d) = v.delta_active.get(t - 1) {
                    obj += c_startup_eur * d;
                }
                if let Some(&d) = v.delta_ramp.get(t - 1) {
                    obj += c_ramp_eur_kw * d;
                }
            }
        }
        obj
    }

    /// Read back the battery solution. Associated function (no `self` needed).
    pub fn read_solution(sol: &impl Solution, v: &BatteryMilpVars, n: usize) -> BatterySolOutput {
        BatterySolOutput {
            p_ch_kw: (0..n).map(|t| sol.value(v.p_ch[t])).collect(),
            p_dis_kw: (0..n).map(|t| sol.value(v.p_dis[t])).collect(),
            e_kwh: (0..=n).map(|t| sol.value(v.e_bat[t])).collect(),
        }
    }
}

impl Battery {
    /// Build the MILP context from the live SoC (not the profile initial_soc).
    pub fn build_milp_context(&self, live_soc: f64) -> BatteryMilpContext {
        let cap = self.capacity_kwh;
        let eff = self.round_trip_efficiency.sqrt();
        BatteryMilpContext {
            e_nom_kwh: cap,
            e_init_kwh: live_soc * cap,
            e_min_kwh: self.min_soc * cap,
            e_max_kwh: cap,
            p_ch_max_kw: self.max_charge_kw,
            p_dis_max_kw: self.max_discharge_kw,
            eff_ch: eff,
            eff_dis: eff,
        }
    }

    /// Declare all LP variables for this battery into `vars`. Delegates to `BatteryMilpContext::declare_vars`.
    pub fn declare_milp_vars(
        &self,
        ctx: &BatteryMilpContext,
        n: usize,
        c_startup_eur: f64,
        c_ramp_eur_kw: f64,
        vars: &mut ProblemVariables,
    ) -> BatteryMilpVars {
        ctx.declare_vars(n, c_startup_eur, c_ramp_eur_kw, vars)
    }

    /// Generate all MILP constraints for this battery. Delegates to `BatteryMilpContext::constraints`.
    pub fn milp_constraints(
        &self,
        ctx: &BatteryMilpContext,
        v: &BatteryMilpVars,
        n: usize,
        dt_h: f64,
    ) -> Vec<Constraint> {
        ctx.constraints(v, n, dt_h)
    }

    /// Battery objective contribution: cycle wear + startup penalty + ramp penalty.
    pub fn milp_objective(
        &self,
        v: &BatteryMilpVars,
        wear_eur_kwh: f64,
        startup_eur: f64,
        ramp_eur_kw: f64,
        n: usize,
        dt_h: f64,
    ) -> Expression {
        BatteryMilpContext::objective(v, wear_eur_kwh, startup_eur, ramp_eur_kw, n, dt_h)
    }

    /// Net battery contribution to the power balance at slot `t`.
    /// Positive = net discharge (supply to the bus), negative = net charge (load on bus).
    pub fn milp_grid_term(&self, t: usize, v: &BatteryMilpVars) -> Expression {
        Expression::from(v.p_dis[t]) - Expression::from(v.p_ch[t])
    }

    /// Read back the battery solution from the solved model. Delegates to `BatteryMilpContext::read_solution`.
    pub fn read_milp_solution(
        &self,
        sol: &impl Solution,
        v: &BatteryMilpVars,
        n: usize,
    ) -> BatterySolOutput {
        BatteryMilpContext::read_solution(sol, v, n)
    }
}

impl Asset for Battery {
    fn step(&self, state: &AssetState, setpoint_kw: f64, dt: Duration) -> (AssetState, f64) {
        let AssetState::Battery(s) = state else {
            unreachable!("Battery/state mismatch")
        };
        let (ns, p) = self.step_inner(s, setpoint_kw, dt);
        (AssetState::Battery(ns), p)
    }

    fn capability(&self, state: &AssetState) -> AssetCapability {
        let AssetState::Battery(s) = state else {
            unreachable!()
        };
        self.capability_inner(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::BatteryConfig;

    fn make_battery_cfg(initial_soc: f64) -> (Battery, BatteryState) {
        let cfg = BatteryConfig {
            id: "battery".to_string(),
            capacity_kwh: 10.0,
            max_charge_kw: 5.0,
            max_discharge_kw: 5.0,
            round_trip_efficiency: 0.95,
            min_soc: 0.1,
            initial_soc,
        };
        (Battery::from_config(&cfg), Battery::initial_state(&cfg))
    }

    #[test]
    fn forecast_zero_timespan_returns_empty() {
        let (bat, state) = make_battery_cfg(0.5);
        let series = bat.forecast(&state, Duration::zero());
        assert!(series.samples.is_empty());
    }

    #[test]
    fn forecast_at_full_soc_charge_setpoint_returns_zero() {
        let (bat, mut state) = make_battery_cfg(1.0);
        state.actual_power_kw = 5.0;
        let series = bat.forecast(&state, Duration::seconds(300));
        for (_, v) in &series.samples {
            assert_eq!(
                *v, 0.0,
                "Full SoC battery with charge setpoint must return zero"
            );
        }
    }

    #[test]
    fn forecast_has_boundary_point() {
        let (bat, state) = make_battery_cfg(0.5);
        let timespan = Duration::seconds(120);
        let before = Utc::now();
        let series = bat.forecast(&state, timespan);
        let after = Utc::now();
        assert!(!series.samples.is_empty());
        let last_ts = series.samples.last().unwrap().0;
        assert!(last_ts >= before + timespan && last_ts <= after + timespan);
    }

    #[test]
    fn step_charges_and_stops_at_full() {
        let (bat, mut state) = make_battery_cfg(0.99);
        for _ in 0..1000 {
            let (ns, _) = bat.step_inner(&state, 10.0, Duration::seconds(1));
            state = ns;
        }
        assert!((state.soc - 1.0).abs() < 0.001);
        let (_, actual) = bat.step_inner(&state, 10.0, Duration::seconds(1));
        assert_eq!(actual, 0.0);
    }
}
