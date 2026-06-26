use chrono::{DateTime, Duration, Utc};
use good_lp::{constraint, variable, Constraint, Expression, ProblemVariables, Solution};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{Asset, AssetCapability, AssetState, ControlDescriptor};
use crate::common::{Interpolation, TimeSeries};
use crate::controller::milp_planner::asset_port::{
    BatteryMilpContext, BatteryMilpVars, BatterySolOutput,
};
use crate::entities::asset_params::BatteryParams;

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
    pub fn from_params(cfg: &BatteryParams) -> Self {
        Self {
            capacity_kwh: cfg.capacity_kwh,
            max_charge_kw: cfg.max_charge_kw,
            max_discharge_kw: cfg.max_discharge_kw,
            round_trip_efficiency: cfg.round_trip_efficiency,
            min_soc: cfg.min_soc,
        }
    }

    pub fn initial_state(cfg: &BatteryParams) -> BatteryState {
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
        let actual = if (clamped > 0.0 && state.soc >= 1.0)
            || (clamped < 0.0 && state.soc <= self.min_soc)
        {
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

    /// State values for a future MILP time slot, given the battery energy stored
    /// at the start of that slot (kWh). Returns `{"soc": <0..1>}`.
    pub fn future_state_values(&self, e_kwh: f64) -> HashMap<String, f64> {
        let soc = (e_kwh / self.capacity_kwh).clamp(0.0, 1.0);
        HashMap::from([("soc".into(), soc)])
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
            let kw = if (setpoint > 0.0 && soc >= 1.0) || (setpoint < 0.0 && soc <= self.min_soc) {
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
            t += Duration::seconds(60);
        }
        let end_kw = if (setpoint > 0.0 && soc >= 1.0) || (setpoint < 0.0 && soc <= self.min_soc) {
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
// Struct definitions live in `controller::milp_planner::asset_port`.
// Method implementations (declare_vars, constraints, objective, read_solution, from_state)
// are in the `impl` blocks below (cross-file inherent impl — valid Rust).

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
    /// `dt_h[t]` is the slot duration in hours for slot `t`.
    pub fn constraints(&self, v: &BatteryMilpVars, n: usize, dt_h: &[f64]) -> Vec<Constraint> {
        let mut cs: Vec<Constraint> = Vec::new();
        for (t, &dt) in dt_h.iter().enumerate().take(n) {
            cs.push(constraint!(v.p_ch[t] <= self.p_ch_max_kw * v.u_bat[t]));
            cs.push(constraint!(
                v.p_dis[t] <= self.p_dis_max_kw * (1.0 - v.u_bat[t])
            ));
            cs.push(constraint!(
                v.e_bat[t + 1]
                    == v.e_bat[t] + dt * self.eff_ch * v.p_ch[t]
                        - dt * (1.0 / self.eff_dis) * v.p_dis[t]
            ));
            if let Some(&z) = v.z_active.get(t) {
                let big_m = self.p_ch_max_kw + self.p_dis_max_kw;
                cs.push(constraint!(v.p_ch[t] + v.p_dis[t] <= big_m * z));
            }
        }
        for i in 0..v.delta_active.len() {
            let t = i + 1;
            cs.push(constraint!(
                v.delta_active[i] >= v.z_active[t] - v.z_active[t - 1]
            ));
        }
        for i in 0..v.delta_ramp.len() {
            let t = i + 1;
            cs.push(constraint!(
                v.delta_ramp[i] >= (v.p_ch[t] - v.p_dis[t]) - (v.p_ch[t - 1] - v.p_dis[t - 1])
            ));
            cs.push(constraint!(
                v.delta_ramp[i] >= (v.p_ch[t - 1] - v.p_dis[t - 1]) - (v.p_ch[t] - v.p_dis[t])
            ));
        }
        cs.push(constraint!(v.e_bat[n] >= self.e_init_kwh));
        cs
    }

    /// Battery objective contribution. Associated function (no `self` needed — no ctx params used).
    /// `dt_h[t]` is the slot duration in hours for slot `t`.
    pub fn objective(
        v: &BatteryMilpVars,
        c_wear_eur_kwh: f64,
        c_startup_eur: f64,
        c_ramp_eur_kw: f64,
        n: usize,
        dt_h: &[f64],
    ) -> Expression {
        let mut obj = Expression::from(0.0);
        for (t, &dt) in dt_h.iter().enumerate().take(n) {
            obj += (c_wear_eur_kwh * dt) * v.p_ch[t];
            obj += (c_wear_eur_kwh * dt) * v.p_dis[t];
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

    /// Construct from a live `AssetState` and the current sim `Battery` config.
    pub fn from_state(state: &super::AssetState, cfg: &Battery, c_terminal_eur_kwh: f64) -> Self {
        let live_soc = if let super::AssetState::Battery(s) = state {
            s.soc
        } else {
            0.5
        };
        cfg.build_milp_context(live_soc, c_terminal_eur_kwh)
    }
}

impl crate::controller::milp_planner::AssetMilpContext for BatteryMilpContext {
    fn asset_id(&self) -> &str {
        "battery"
    }

    fn asset_kind(&self) -> crate::controller::milp_planner::AssetKind {
        crate::controller::milp_planner::AssetKind::Battery
    }

    fn milp_params(
        &self,
        _n: usize,
        _step_s: u64,
        _now: chrono::DateTime<chrono::Utc>,
    ) -> crate::controller::milp_planner::AssetMilpParams {
        crate::controller::milp_planner::AssetMilpParams::Battery(
            crate::controller::milp_planner::BatteryScalars {
                e_nom_kwh: self.e_nom_kwh,
                e_init_kwh: self.e_init_kwh,
                e_min_kwh: self.e_min_kwh,
                e_max_kwh: self.e_max_kwh,
                p_ch_max_kw: self.p_ch_max_kw,
                p_dis_max_kw: self.p_dis_max_kw,
                eff_ch: self.eff_ch,
                eff_dis: self.eff_dis,
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
        pool.bat = Some(self.declare_vars(n, c_startup_eur, c_ramp_eur_kw, vars));
    }

    fn constraints(
        &self,
        pool: &crate::controller::milp_interactions::MilpVarPool,
        n: usize,
        dt_h: &[f64],
    ) -> Vec<Constraint> {
        BatteryMilpContext::constraints(self, pool.bat.as_ref().unwrap(), n, dt_h)
    }

    fn objective(
        &self,
        pool: &crate::controller::milp_interactions::MilpVarPool,
        n: usize,
        dt_h: &[f64],
        c_wear_eur_kwh: f64,
        c_startup_eur: f64,
        c_ramp_eur_kw: f64,
    ) -> Expression {
        let v = pool.bat.as_ref().unwrap();
        let mut obj =
            BatteryMilpContext::objective(v, c_wear_eur_kwh, c_startup_eur, c_ramp_eur_kw, n, dt_h);
        // Terminal energy reward in Phase 1 only (c_startup_eur == 0.0).
        // e_bat[n] is the SoC trajectory end-state (index n+1 of the n+1 vector).
        if c_startup_eur == 0.0 && self.c_terminal_eur_kwh > 0.0 && n > 0 {
            obj += -self.c_terminal_eur_kwh * v.e_bat[n];
        }
        obj
    }
}

impl Battery {
    /// Build the MILP context from the live SoC (not the profile initial_soc).
    pub fn build_milp_context(&self, live_soc: f64, c_terminal_eur_kwh: f64) -> BatteryMilpContext {
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
            c_terminal_eur_kwh,
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
        dt_h: &[f64],
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
        dt_h: &[f64],
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

    fn make_battery_cfg(initial_soc: f64) -> (Battery, BatteryState) {
        let cfg = BatteryParams {
            id: "battery".to_string(),
            capacity_kwh: 10.0,
            max_charge_kw: 5.0,
            max_discharge_kw: 5.0,
            round_trip_efficiency: 0.95,
            min_soc: 0.1,
            initial_soc,
            c_terminal_eur_kwh: None,
        };
        (Battery::from_params(&cfg), Battery::initial_state(&cfg))
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

    // T007: Battery::future_state_values returns correct soc.
    #[test]
    fn future_state_values_mid_charge() {
        let bat = Battery {
            capacity_kwh: 10.0,
            max_charge_kw: 5.0,
            max_discharge_kw: 5.0,
            round_trip_efficiency: 0.9,
            min_soc: 0.1,
        };
        let vals = bat.future_state_values(5.0); // 5 kWh of 10 kWh capacity → SoC = 0.5
        let soc = vals["soc"];
        assert!((soc - 0.5).abs() < 1e-9, "expected soc=0.5, got {soc}");
    }

    #[test]
    fn future_state_values_clamps_to_zero() {
        let bat = Battery {
            capacity_kwh: 10.0,
            max_charge_kw: 5.0,
            max_discharge_kw: 5.0,
            round_trip_efficiency: 0.9,
            min_soc: 0.1,
        };
        let vals = bat.future_state_values(-1.0);
        assert_eq!(vals["soc"], 0.0);
    }

    #[test]
    fn future_state_values_clamps_to_one() {
        let bat = Battery {
            capacity_kwh: 10.0,
            max_charge_kw: 5.0,
            max_discharge_kw: 5.0,
            round_trip_efficiency: 0.9,
            min_soc: 0.1,
        };
        let vals = bat.future_state_values(15.0);
        assert_eq!(vals["soc"], 1.0);
    }
}

#[cfg(test)]
mod param_tests {
    use super::*;

    #[test]
    fn battery_params_default_soc() {
        assert!((BatteryParams::default().initial_soc - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn battery_params_custom_capacity() {
        let params = BatteryParams {
            capacity_kwh: 20.0,
            ..BatteryParams::default()
        };
        assert!((params.capacity_kwh - 20.0).abs() < f64::EPSILON);
    }
}

#[cfg(test)]
mod milp_context_trait_tests {
    use super::*;
    use crate::controller::milp_interactions::{GridMilpVars, MilpVarPool};
    use crate::controller::milp_planner::{AssetKind, AssetMilpContext, AssetMilpParams};
    use good_lp::{variable, variables};

    fn make_ctx() -> BatteryMilpContext {
        BatteryMilpContext {
            e_nom_kwh: 10.0,
            e_init_kwh: 5.0,
            e_min_kwh: 1.0,
            e_max_kwh: 10.0,
            p_ch_max_kw: 5.0,
            p_dis_max_kw: 5.0,
            eff_ch: 0.9746794_f64.sqrt(),
            eff_dis: 0.9746794_f64.sqrt(),
            c_terminal_eur_kwh: 0.0,
        }
    }

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

    #[test]
    fn asset_id_is_battery() {
        assert_eq!(make_ctx().asset_id(), "battery");
    }

    #[test]
    fn asset_kind_is_battery() {
        assert_eq!(make_ctx().asset_kind(), AssetKind::Battery);
    }

    #[test]
    fn milp_params_returns_correct_battery_scalars() {
        let ctx = make_ctx();
        let params = ctx.milp_params(4, 300, chrono::Utc::now());
        match params {
            AssetMilpParams::Battery(b) => {
                assert!((b.e_nom_kwh - 10.0).abs() < 1e-9);
                assert!((b.e_init_kwh - 5.0).abs() < 1e-9);
                assert!((b.e_min_kwh - 1.0).abs() < 1e-9);
                assert!((b.p_ch_max_kw - 5.0).abs() < 1e-9);
                assert!((b.p_dis_max_kw - 5.0).abs() < 1e-9);
            }
            _ => panic!("expected AssetMilpParams::Battery"),
        }
    }

    #[test]
    fn declare_vars_into_pool_fills_bat_slot() {
        let ctx = make_ctx();
        let n = 4;
        let mut vars = variables!();
        let mut pool = empty_pool(&mut vars, n);
        ctx.declare_vars_into_pool(n, 0.0, 0.0, &mut vars, &mut pool);
        let v = pool
            .bat
            .as_ref()
            .expect("pool.bat should be Some after declare");
        assert_eq!(v.p_ch.len(), n);
        assert_eq!(v.p_dis.len(), n);
        assert_eq!(v.e_bat.len(), n + 1);
        assert!(v.z_active.is_empty()); // no startup vars when c_startup=0
        assert!(v.delta_ramp.is_empty()); // no ramp vars when c_ramp=0
    }

    #[test]
    fn constraints_non_empty_for_n4() {
        let ctx = make_ctx();
        let n = 4;
        let dt_h = vec![300.0 / 3600.0; n];
        let mut vars = variables!();
        let mut pool = empty_pool(&mut vars, n);
        ctx.declare_vars_into_pool(n, 0.0, 0.0, &mut vars, &mut pool);
        let cs = AssetMilpContext::constraints(&ctx, &pool, n, &dt_h);
        assert!(
            cs.len() >= n * 3 + 1,
            "expected at least {} constraints, got {}",
            n * 3 + 1,
            cs.len()
        );
    }
}
