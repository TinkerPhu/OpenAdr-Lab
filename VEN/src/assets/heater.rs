use chrono::{DateTime, Duration, Utc};
use good_lp::{constraint, variable, Constraint, Expression, ProblemVariables, Solution, Variable};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{
    Asset, AssetCapabilities, AssetCapability, AssetState, ControlDescriptor, ControlKind,
};
use crate::common::{Interpolation, TimeSeries};
use crate::profile::HeaterConfig;

/// Heater config. Consumes power for space heating (positive = import).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Heater {
    pub max_kw: f64,
    /// Forced-on floor power at temp_min_c (0.0 if none).
    pub min_power_kw: f64,
    /// Tank hysteresis lower bound. Overridable at runtime via SimInjectState.
    pub temp_min_c: f64,
    /// Tank hysteresis upper bound. Overridable at runtime via SimInjectState.
    pub temp_max_c: f64,
    /// Original profile value — used for snap-back when inject override is released.
    pub temp_min_c_profile: f64,
    /// Original profile value — used for snap-back when inject override is released.
    pub temp_max_c_profile: f64,
    /// Thermal mass in kWh/°C. Derived from volume_l (water tank) or explicit config.
    pub thermal_mass_kwh_per_c: f64,
    /// Newton cooling coefficient (kW/°C). Loss = k_loss × (temp − ambient).
    pub k_loss_kw_per_c: f64,
    /// Constant simulated hot water draw (kW thermal). Defaults to 0.0.
    pub draw_kw: f64,
    /// Set each tick by sim from SimInjectState.ambient_temp_c; NOT from YAML.
    pub ambient_temp_c: f64,
}

/// Heater mutable state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaterState {
    pub temperature_c: f64,
    /// Actual power last tick. Always ≥ 0 (heaters only consume).
    pub actual_power_kw: f64,
}

impl Heater {
    pub fn from_config(cfg: &HeaterConfig) -> Self {
        Self {
            max_kw: cfg.max_kw,
            min_power_kw: 0.0,
            temp_min_c: cfg.temp_min_c,
            temp_max_c: cfg.temp_max_c,
            temp_min_c_profile: cfg.temp_min_c,
            temp_max_c_profile: cfg.temp_max_c,
            thermal_mass_kwh_per_c: cfg.effective_thermal_mass(),
            k_loss_kw_per_c: cfg.effective_k_loss(),
            draw_kw: cfg.effective_draw_kw(),
            ambient_temp_c: 10.0,
        }
    }

    pub fn initial_state(cfg: &HeaterConfig) -> HeaterState {
        HeaterState {
            temperature_c: cfg.temp_initial_c,
            actual_power_kw: 0.0,
        }
    }

    /// Pure physics step. Returns (new_state, actual_power_kw).
    /// Reads `self.ambient_temp_c` (set by sim loop each tick before calling).
    pub fn step_inner(
        &self,
        state: &HeaterState,
        setpoint_kw: f64,
        dt: Duration,
    ) -> (HeaterState, f64) {
        let dt_h = dt.num_milliseconds() as f64 / 3_600_000.0;
        let clamped = setpoint_kw.clamp(0.0, self.max_kw);
        // Thermostat overrides
        let actual = if state.temperature_c >= self.temp_max_c {
            0.0
        } else if state.temperature_c <= self.temp_min_c {
            // Emergency: ignore setpoint and run at max to recover temperature.
            self.max_kw
        } else {
            clamped
        };
        // Thermal model: Newton cooling + simulated draw
        let loss_kw = (state.temperature_c - self.ambient_temp_c) * self.k_loss_kw_per_c;
        let delta_c = (actual - loss_kw - self.draw_kw) / self.thermal_mass_kwh_per_c * dt_h;
        let new_temp = state.temperature_c + delta_c;
        (
            HeaterState {
                temperature_c: new_temp,
                actual_power_kw: actual,
            },
            actual,
        )
    }

    /// Point-in-time feasible power range.
    pub fn capability_inner(&self, state: &HeaterState) -> AssetCapability {
        let max_import_kw = if state.temperature_c >= self.temp_max_c {
            0.0 // overheat — forced off
        } else if state.temperature_c <= self.temp_min_c {
            self.min_power_kw // too cold — forced on at minimum power
        } else {
            self.max_kw
        };
        AssetCapability {
            max_export_kw: 0.0,
            max_import_kw,
        }
    }

    pub fn default_setpoint(&self) -> f64 {
        self.max_kw * 0.5
    }

    pub fn state_values(&self, state: &HeaterState) -> HashMap<String, f64> {
        let mut m = HashMap::new();
        m.insert("temp_c".into(), state.temperature_c);
        m.insert("max_kw".into(), self.max_kw);
        m.insert("temp_min_c".into(), self.temp_min_c);
        m.insert("temp_max_c".into(), self.temp_max_c);
        m
    }

    pub fn capabilities(&self, asset_id: &str, _state: &HeaterState) -> AssetCapabilities {
        AssetCapabilities {
            asset_id: asset_id.to_string(),
            max_import_kw: self.max_kw,
            max_export_kw: 0.0,
            is_flexible: true,
            energy_state: None,
            availability: None,
        }
    }

    pub fn control_schema(&self) -> Vec<ControlDescriptor> {
        // heater_temp_c (T_tank one-shot reset) and heater_setpoint_c (kW power
        // override) are handled by dedicated UI elements, not schema-driven controls.
        vec![
            ControlDescriptor {
                key: "heater_temp_min_c".into(),
                label: "T_tank_min".into(),
                kind: ControlKind::Slider,
                min: Some(18.0),
                max: Some(94.0),
                unit: "°C".into(),
                display_scale: None,
            },
            ControlDescriptor {
                key: "heater_temp_max_c".into(),
                label: "T_tank_max".into(),
                kind: ControlKind::Slider,
                min: Some(19.0),
                max: Some(95.0),
                unit: "°C".into(),
                display_scale: None,
            },
        ]
    }

    pub fn reset(&self, state: &mut HeaterState, values: HashMap<String, f64>) {
        if let Some(&t) = values.get("temp_c") {
            state.temperature_c = t;
        }
    }

    pub fn update_config(&mut self, values: HashMap<String, f64>) {
        if let Some(&v) = values.get("max_kw") {
            self.max_kw = v.max(0.0);
        }
    }

    pub fn forecast(&self, state: &HeaterState, timespan: Duration) -> TimeSeries {
        if timespan <= Duration::zero() {
            return TimeSeries::empty(Interpolation::Linear);
        }
        let now = Utc::now();
        let end = now + timespan;
        // Use the design setpoint (max_kw * 0.5) so the forecast reflects the
        // thermostat's long-run equilibrium (~1.3 kW at nominal ambient), not the
        // instantaneous actual_power_kw which is 0 whenever the thermostat forces off.
        let setpoint = self.default_setpoint();
        let mut samples: Vec<(DateTime<Utc>, f64)> = Vec::new();

        let mut t = now;
        let mut temp = state.temperature_c;

        while t < end {
            let dt_h = 1.0 / 60.0;
            let loss_kw = (temp - self.ambient_temp_c) * self.k_loss_kw_per_c;
            let kw = if temp < self.temp_min_c {
                self.max_kw
            } else if temp > self.temp_max_c {
                0.0
            } else {
                setpoint
            };
            samples.push((t, kw));
            let net_kwh = (kw - loss_kw - self.draw_kw) * dt_h;
            temp += net_kwh / self.thermal_mass_kwh_per_c;
            t = t + Duration::seconds(60);
        }
        let end_kw = if temp < self.temp_min_c {
            self.max_kw
        } else if temp > self.temp_max_c {
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
                max_marginal_price: 0.30,
                max_marginal_co2: 0.0,
            },
            crate::entities::asset::ComfortRate {
                fill: 1.0,
                max_marginal_price: 0.10,
                max_marginal_co2: 0.0,
            },
        ]
    }

    pub fn default_completion_policy(&self) -> crate::entities::asset::CompletionPolicy {
        crate::entities::asset::CompletionPolicy::Continue
    }

    pub fn default_post_deadline_comfort_bid(&self) -> Option<f64> {
        Some(0.10)
    }
}

impl Asset for Heater {
    fn step(&self, state: &AssetState, setpoint_kw: f64, dt: Duration) -> (AssetState, f64) {
        let AssetState::Heater(s) = state else {
            unreachable!("Heater/state mismatch")
        };
        let (ns, p) = self.step_inner(s, setpoint_kw, dt);
        (AssetState::Heater(ns), p)
    }

    fn capability(&self, state: &AssetState) -> AssetCapability {
        let AssetState::Heater(s) = state else {
            unreachable!()
        };
        self.capability_inner(s)
    }
}

// ── Heater MILP plugin types ──────────────────────────────────────────────────

/// Scheduling mode for the heater in the MILP model.
#[derive(Debug, Clone, PartialEq)]
pub enum HeaterMilpMode {
    /// Hard energy target — E[t_dead] ≥ e_target_kwh must hold at the deadline.
    MustRun,
    /// Opportunistic — scheduled by tariffs; soft deadline reward via z_heat_ready.
    MayRun,
    /// Heater absent — all power variables fixed to zero.
    MustNotRun,
}

/// Pre-computed MILP parameters for one heater and planning cycle.
/// Uses a per-step tank energy state trajectory (E[t]) instead of a global energy budget.
#[derive(Debug, Clone)]
pub struct HeaterMilpContext {
    pub mode: HeaterMilpMode,
    /// Deadline step index (None = no hard deadline; autonomous MayRun path).
    pub t_dead_step: Option<usize>,
    /// Mid power level [kW].
    pub p_mid_kw: f64,
    /// Full power level [kW] = max_kw.
    pub p_full_kw: f64,
    /// Initial tank energy above T_min [kWh]. May be negative when tank is below T_min.
    pub e_init_kwh: f64,
    /// Maximum usable tank energy above T_min [kWh] = (T_max − T_min) × thermal_mass.
    pub e_max_kwh: f64,
    /// Constant per-step thermal demand [kW]: draw_kw + k_loss × (T_mid − ambient).
    pub q_dem_kw: f64,
    /// Target tank energy at deadline [kWh above T_min]. = e_max_kwh in autonomous mode.
    pub e_target_kwh: f64,
    /// Relay switching penalty [EUR/switch event] added to the objective.
    pub lambda_sw_eur: f64,
}

/// Typed LP variable handles for one heater in the MILP model.
#[derive(Debug, Clone)]
pub struct HeaterMilpVars {
    /// Binary: 1 = mid power tier active at slot t. len = n.
    pub z_heat_mid: Vec<Variable>,
    /// Binary: 1 = full power tier active at slot t. len = n.
    pub z_heat_full: Vec<Variable>,
    /// Binary: 1 when deadline is met (MayRun only; fixed 0 in MustRun / autonomous).
    pub z_heat_ready: Variable,
    /// Continuous: tank energy above T_min [kWh] at slot t. Domain [−e_max, e_max]. len = n.
    pub e_tank: Vec<Variable>,
    /// Continuous ≥ 0: below-minimum soft-violation slack [kWh] at slot t. len = n.
    pub s_low: Vec<Variable>,
    /// Continuous ≥ 0: switching indicator per step (sw[0] fixed at 0). len = n.
    pub sw: Vec<Variable>,
}

/// Per-heater MILP solution readback.
#[derive(Debug, Clone)]
pub struct HeaterSolOutput {
    pub z_heat_mid: Vec<f64>,
    pub z_heat_full: Vec<f64>,
    pub z_heat_ready: f64,
    /// Tank energy above T_min [kWh] per slot. len = n.
    pub e_tank_kwh: Vec<f64>,
    /// Below-min slack [kWh] per slot. len = n.
    pub s_low_kwh: Vec<f64>,
    /// Switching cost contribution per step. len = n.
    pub sw: Vec<f64>,
}

impl HeaterMilpContext {
    /// Declare all LP variables for this heater.
    pub fn declare_vars(&self, n: usize, vars: &mut ProblemVariables) -> HeaterMilpVars {
        let must_not = self.mode == HeaterMilpMode::MustNotRun;

        let z_heat_mid = (0..n)
            .map(|_| {
                if must_not { vars.add(variable().min(0.0).max(0.0)) }
                else { vars.add(variable().binary()) }
            })
            .collect();
        let z_heat_full = (0..n)
            .map(|_| {
                if must_not { vars.add(variable().min(0.0).max(0.0)) }
                else { vars.add(variable().binary()) }
            })
            .collect();

        // z_heat_ready: binary reward flag for MayRun with deadline; fixed 0 otherwise.
        let z_heat_ready = if self.mode == HeaterMilpMode::MayRun && self.t_dead_step.is_some() {
            vars.add(variable().binary())
        } else {
            vars.add(variable().min(0.0).max(0.0))
        };

        // e_tank[t]: continuous tank energy above T_min [kWh], domain [−e_max, e_max].
        let e_lo = -self.e_max_kwh.max(1.0); // allow negative (below T_min)
        let e_hi = self.e_max_kwh.max(1.0);
        let e_tank = (0..n)
            .map(|_| vars.add(variable().min(e_lo).max(e_hi)))
            .collect();

        // s_low[t]: non-negative below-min slack.
        let s_low = (0..n)
            .map(|_| vars.add(variable().min(0.0)))
            .collect();

        // sw[0]: fixed at 0 (no previous slot); sw[t>0]: continuous ≥ 0.
        let sw = (0..n)
            .map(|t| {
                if t == 0 { vars.add(variable().min(0.0).max(0.0)) }
                else { vars.add(variable().min(0.0)) }
            })
            .collect();

        HeaterMilpVars { z_heat_mid, z_heat_full, z_heat_ready, e_tank, s_low, sw }
    }

    /// Instantaneous heater power expression at slot `t` (for power balance).
    pub fn power_expr(&self, v: &HeaterMilpVars, t: usize) -> Expression {
        self.p_mid_kw * v.z_heat_mid[t] + self.p_full_kw * v.z_heat_full[t]
    }

    /// Generate all MILP constraints for this heater.
    pub fn constraints(&self, v: &HeaterMilpVars, n: usize, dt_h: f64) -> Vec<Constraint> {
        let mut cs: Vec<Constraint> = Vec::new();

        // C0: mutual exclusion — mid and full are alternative modes.
        for t in 0..n {
            cs.push(constraint!(v.z_heat_mid[t] + v.z_heat_full[t] <= 1.0));
        }

        if self.mode == HeaterMilpMode::MustNotRun {
            return cs; // all power vars already fixed to 0; no trajectory needed
        }

        // C1: pin initial tank energy.
        let e_init = self.e_init_kwh;
        cs.push(constraint!(v.e_tank[0] >= e_init));
        cs.push(constraint!(v.e_tank[0] <= e_init));

        // C2: tank dynamics — E[t+1] = E[t] + (P_heat[t] − q_dem) × dt_h.
        // Expressed as two inequalities (== is not directly supported).
        let net_const = -self.q_dem_kw * dt_h;
        for t in 0..(n.saturating_sub(1)) {
            let p_mid_dt = self.p_mid_kw * dt_h;
            let p_full_dt = self.p_full_kw * dt_h;
            // LHS = e_tank[t+1] − e_tank[t] − p_mid_dt×z_mid[t] − p_full_dt×z_full[t]
            let lhs_ge = Expression::from(v.e_tank[t + 1])
                - Expression::from(v.e_tank[t])
                - p_mid_dt * v.z_heat_mid[t]
                - p_full_dt * v.z_heat_full[t];
            cs.push(constraint!(lhs_ge >= net_const));
            let lhs_le = Expression::from(v.e_tank[t + 1])
                - Expression::from(v.e_tank[t])
                - p_mid_dt * v.z_heat_mid[t]
                - p_full_dt * v.z_heat_full[t];
            cs.push(constraint!(lhs_le <= net_const));
        }

        // C3: upper bound — no overheating.
        for t in 0..n {
            cs.push(constraint!(v.e_tank[t] <= self.e_max_kwh));
        }

        // C4: soft lower bound — penalise going below T_min.
        for t in 0..n {
            cs.push(constraint!(v.e_tank[t] + v.s_low[t] >= 0.0));
        }

        // C5: switching indicators — sw[t] ≥ |z_x[t] − z_x[t−1]| for each binary.
        for t in 1..n {
            cs.push(constraint!(v.sw[t] >= v.z_heat_mid[t] - v.z_heat_mid[t - 1]));
            cs.push(constraint!(v.sw[t] >= v.z_heat_mid[t - 1] - v.z_heat_mid[t]));
            cs.push(constraint!(v.sw[t] >= v.z_heat_full[t] - v.z_heat_full[t - 1]));
            cs.push(constraint!(v.sw[t] >= v.z_heat_full[t - 1] - v.z_heat_full[t]));
        }

        // C6: deadline constraint.
        if let Some(td) = self.t_dead_step {
            let td = td.min(n.saturating_sub(1));
            match self.mode {
                HeaterMilpMode::MustRun => {
                    cs.push(constraint!(v.e_tank[td] >= self.e_target_kwh));
                }
                HeaterMilpMode::MayRun => {
                    // e_tank[td] ≥ e_target × z_heat_ready (linear: e_target is a scalar)
                    let rhs = self.e_target_kwh * v.z_heat_ready;
                    cs.push(constraint!(v.e_tank[td] >= rhs));
                }
                HeaterMilpMode::MustNotRun => {}
            }
        }

        cs
    }

    /// Heater objective contribution (penalty terms only; energy cost enters via power balance).
    pub fn objective(
        &self,
        v: &HeaterMilpVars,
        w_tier_penalty_eur: f64,
        m_low_eur_kwh: f64,
        n: usize,
    ) -> Expression {
        let mut obj = Expression::from(0.0);
        if self.mode == HeaterMilpMode::MustNotRun {
            return obj;
        }
        for t in 0..n {
            obj += w_tier_penalty_eur * v.z_heat_full[t]; // prefer mid over full when equal cost
            obj += m_low_eur_kwh * v.s_low[t];            // penalise below-min violations
            obj += self.lambda_sw_eur * v.sw[t];           // penalise relay switches
        }
        obj
    }

    /// Read back the heater solution from the solved model.
    pub fn read_solution(sol: &impl Solution, v: &HeaterMilpVars, n: usize) -> HeaterSolOutput {
        HeaterSolOutput {
            z_heat_mid: (0..n).map(|t| sol.value(v.z_heat_mid[t])).collect(),
            z_heat_full: (0..n).map(|t| sol.value(v.z_heat_full[t])).collect(),
            z_heat_ready: sol.value(v.z_heat_ready),
            e_tank_kwh: (0..n).map(|t| sol.value(v.e_tank[t])).collect(),
            s_low_kwh: (0..n).map(|t| sol.value(v.s_low[t])).collect(),
            sw: (0..n).map(|t| sol.value(v.sw[t])).collect(),
        }
    }
}

impl Heater {
    /// Constant per-step thermal demand forecast [kW].
    /// Uses the midpoint of the comfort band as the representative tank temperature.
    /// `Q_dem = draw_kw + k_loss × (T_mid − ambient_temp_c)`
    pub fn forecast_demand_kw(&self, ambient_temp_c: f64) -> f64 {
        let t_mid = (self.temp_min_c + self.temp_max_c) / 2.0;
        (self.draw_kw + self.k_loss_kw_per_c * (t_mid - ambient_temp_c)).max(0.0)
    }

    /// Declare all LP variables for this heater into `vars`. Delegates to `HeaterMilpContext::declare_vars`.
    pub fn declare_milp_vars(
        &self,
        ctx: &HeaterMilpContext,
        n: usize,
        vars: &mut ProblemVariables,
    ) -> HeaterMilpVars {
        ctx.declare_vars(n, vars)
    }

    /// Instantaneous heater power expression at slot `t`. Delegates to `HeaterMilpContext::power_expr`.
    pub fn milp_power_expr(
        &self,
        ctx: &HeaterMilpContext,
        v: &HeaterMilpVars,
        t: usize,
    ) -> Expression {
        ctx.power_expr(v, t)
    }

    /// Generate all MILP constraints for this heater. Delegates to `HeaterMilpContext::constraints`.
    pub fn milp_constraints(
        &self,
        ctx: &HeaterMilpContext,
        v: &HeaterMilpVars,
        n: usize,
        dt_h: f64,
    ) -> Vec<Constraint> {
        ctx.constraints(v, n, dt_h)
    }

    /// Heater objective contribution. Delegates to `HeaterMilpContext::objective`.
    pub fn milp_objective(
        &self,
        ctx: &HeaterMilpContext,
        v: &HeaterMilpVars,
        w_tier_penalty_eur: f64,
        m_low_eur_kwh: f64,
        n: usize,
    ) -> Expression {
        ctx.objective(v, w_tier_penalty_eur, m_low_eur_kwh, n)
    }

    /// Read back the heater solution from the solved model. Delegates to `HeaterMilpContext::read_solution`.
    pub fn read_milp_solution(
        &self,
        sol: &impl Solution,
        v: &HeaterMilpVars,
        n: usize,
    ) -> HeaterSolOutput {
        HeaterMilpContext::read_solution(sol, v, n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_heater() -> Heater {
        Heater {
            max_kw: 2.5,
            min_power_kw: 0.0,
            temp_min_c: 20.0,
            temp_max_c: 23.0,
            temp_min_c_profile: 20.0,
            temp_max_c_profile: 23.0,
            thermal_mass_kwh_per_c: 2.0,
            k_loss_kw_per_c: 0.1,
            draw_kw: 0.0,
            ambient_temp_c: 10.0,
        }
    }

    /// Hot water tank fixture: 200 L, 40–80 °C comfort band, low heat loss, 0.5 kW draw.
    fn hot_water_heater() -> Heater {
        Heater {
            max_kw: 3.0,
            min_power_kw: 0.0,
            temp_min_c: 40.0,
            temp_max_c: 80.0,
            temp_min_c_profile: 40.0,
            temp_max_c_profile: 80.0,
            thermal_mass_kwh_per_c: 200.0 * 4.186 / 3600.0, // ≈ 0.233 kWh/°C
            k_loss_kw_per_c: 0.003,
            draw_kw: 0.5,
            ambient_temp_c: 20.0,
        }
    }

    fn state_at(temperature_c: f64, actual_power_kw: f64) -> HeaterState {
        HeaterState {
            temperature_c,
            actual_power_kw,
        }
    }

    // ── control_schema ────────────────────────────────────────────────────────

    #[test]
    fn control_schema_returns_two_descriptors() {
        let heater = default_heater();
        let schema = heater.control_schema();
        let keys: Vec<_> = schema.iter().map(|d| d.key.as_str()).collect();
        assert!(keys.contains(&"heater_temp_min_c"), "missing heater_temp_min_c");
        assert!(keys.contains(&"heater_temp_max_c"), "missing heater_temp_max_c");
        assert_eq!(schema.len(), 2, "expected exactly 2 control descriptors");
    }

    #[test]
    fn control_schema_t_tank_bounds_are_18_to_95() {
        let heater = default_heater();
        let schema = heater.control_schema();
        let min_d = schema.iter().find(|d| d.key == "heater_temp_min_c").unwrap();
        let max_d = schema.iter().find(|d| d.key == "heater_temp_max_c").unwrap();
        assert_eq!(min_d.min.unwrap(), 18.0);
        assert_eq!(min_d.max.unwrap(), 94.0);
        assert_eq!(max_d.min.unwrap(), 19.0);
        assert_eq!(max_d.max.unwrap(), 95.0);
        assert_eq!(min_d.label, "T_tank_min");
        assert_eq!(max_d.label, "T_tank_max");
    }

    // ── forecast ─────────────────────────────────────────────────────────────

    /// When the heater is at temp_max (thermostat forced off), actual_power_kw=0.
    /// The OLD code would use 0 as the setpoint, producing ~0 kW average forecast.
    /// The NEW code uses default_setpoint() = max_kw * 0.5 = 1.25 kW, giving a
    /// realistic oscillating forecast converging to the thermal equilibrium.
    #[test]
    fn forecast_at_temp_max_gives_non_zero_average_power() {
        let heater = default_heater();
        // Heater at ceiling: old code → forecast setpoint = 0 → ~0 kW average.
        let state = state_at(23.0, 0.0);
        let ts = heater.forecast(&state, Duration::hours(2));

        // Compute mean power over the forecast samples
        let n = ts.samples.len() as f64;
        assert!(n > 0.0, "forecast produced no samples");
        let mean: f64 = ts.samples.iter().map(|(_, kw)| kw).sum::<f64>() / n;

        // Thermal equilibrium at ambient=10°C, temp_max=23°C → loss = 1.3 kW.
        // Allow ±0.5 kW tolerance for simulation step error.
        assert!(
            mean > 0.5,
            "forecast mean {mean:.3} kW is too close to 0 — old bug likely present",
        );
        assert!(
            mean < 2.5,
            "forecast mean {mean:.3} kW exceeds max_kw — something is wrong",
        );
    }

    /// When actual_power_kw is already non-zero, both old and new code produce
    /// similar results, but new code is consistent.
    #[test]
    fn forecast_at_mid_temp_gives_reasonable_oscillation() {
        let heater = default_heater();
        let state = state_at(21.5, 1.3);
        let ts = heater.forecast(&state, Duration::hours(1));
        let n = ts.samples.len() as f64;
        assert!(n > 0.0);
        let mean: f64 = ts.samples.iter().map(|(_, kw)| kw).sum::<f64>() / n;
        // Expect long-run equilibrium in reasonable range
        assert!((0.5..=2.5).contains(&mean), "mean {mean:.3} kW out of range");
    }

    // ── step_inner physics ────────────────────────────────────────────────────

    #[test]
    fn heater_turns_off_above_temp_max() {
        let heater = default_heater();
        let state = state_at(23.1, 2.5);
        let (_ns, power) = heater.step_inner(&state, 2.5, Duration::seconds(1));
        assert_eq!(power, 0.0, "heater must be forced off above temp_max");
    }

    #[test]
    fn heater_turns_on_below_temp_min() {
        let heater = default_heater();
        let state = state_at(19.9, 0.0);
        let (_ns, power) = heater.step_inner(&state, 1.0, Duration::seconds(1));
        assert_eq!(power, heater.max_kw, "heater must run at max_kw below temp_min");
    }

    #[test]
    fn heater_follows_setpoint_in_comfort_band() {
        let heater = default_heater();
        let state = state_at(21.5, 0.0);
        let setpoint = 1.5;
        let (_ns, power) = heater.step_inner(&state, setpoint, Duration::seconds(1));
        assert!((power - setpoint).abs() < 1e-9, "heater should follow setpoint");
    }

    // ── hot water tank physics ────────────────────────────────────────────────

    #[test]
    fn hwt_uses_configurable_k_loss() {
        // k_loss = 0.003 kW/°C; at 60°C ambient=20°C → loss = (60-20)*0.003 = 0.12 kW
        let heater = hot_water_heater();
        let state = state_at(60.0, 0.0);
        // setpoint = 0 → heater off (in comfort band 40–80°C)
        let (new_state, power) = heater.step_inner(&state, 0.0, Duration::seconds(3600));
        assert_eq!(power, 0.0);
        // In 1 h at 0 kW, 0.12 kW draw subtracted: net = 0 - 0.12 - 0.5 = -0.62 kW
        // delta_c = -0.62 / 0.233 = -2.66 °C  (roughly)
        let expected_loss = (60.0 - 20.0) * 0.003 + 0.5; // loss + draw
        let expected_delta = -expected_loss / (200.0 * 4.186 / 3600.0);
        let actual_delta = new_state.temperature_c - 60.0;
        assert!(
            (actual_delta - expected_delta).abs() < 0.01,
            "k_loss or draw physics wrong: got Δ{:.3}°C, expected Δ{:.3}°C",
            actual_delta, expected_delta
        );
    }

    #[test]
    fn hwt_draw_drains_tank_when_off() {
        // With 0.5 kW draw and no heater, tank should cool faster than without draw.
        let heater = hot_water_heater();
        let no_draw = Heater { draw_kw: 0.0, ..hot_water_heater() };
        let state = state_at(60.0, 0.0);
        let dt = Duration::seconds(3600);
        let (s_with_draw, _) = heater.step_inner(&state, 0.0, dt);
        let (s_no_draw, _) = no_draw.step_inner(&state, 0.0, dt);
        assert!(
            s_with_draw.temperature_c < s_no_draw.temperature_c,
            "draw should cause faster cooling"
        );
    }

    #[test]
    fn hwt_heats_slowly_with_low_k_loss() {
        // With k_loss=0.003, a 3 kW heater at 60°C and 20°C ambient
        // should heat the 0.233 kWh/°C tank by ~ (3 - 0.12 - 0.5) * 1h / 0.233 ≈ 10.2°C/h
        let heater = hot_water_heater();
        let state = state_at(60.0, 3.0);
        let (new_state, _) = heater.step_inner(&state, 3.0, Duration::seconds(3600));
        let delta = new_state.temperature_c - 60.0;
        assert!(
            delta > 5.0 && delta < 20.0,
            "tank should heat 5–20°C in 1h with 3kW; got {:.2}°C",
            delta
        );
    }

    #[test]
    fn hwt_emergency_on_below_temp_min() {
        let heater = hot_water_heater();
        let state = state_at(39.9, 0.0); // just below min (40°C)
        let (_ns, power) = heater.step_inner(&state, 0.0, Duration::seconds(1));
        assert_eq!(power, heater.max_kw, "emergency: must run at max below temp_min");
    }

    #[test]
    fn hwt_forced_off_above_temp_max() {
        let heater = hot_water_heater();
        let state = state_at(80.1, 3.0);
        let (_ns, power) = heater.step_inner(&state, 3.0, Duration::seconds(1));
        assert_eq!(power, 0.0, "must be forced off above temp_max");
    }

    // ── HeaterMilpContext trajectory model unit tests ─────────────────────────

    #[test]
    fn forecast_demand_kw_equals_draw_plus_loss_at_midpoint() {
        // forecast_demand_kw(ambient) = draw_kw + k_loss × (T_mid − ambient)
        // T_mid = (40+80)/2 = 60; ambient = 20; draw = 0.5; k_loss = 0.003
        // expected: 0.5 + 0.003 × (60 − 20) = 0.62 kW
        let heater = hot_water_heater();
        let q_dem = heater.forecast_demand_kw(20.0);
        assert!((q_dem - 0.62).abs() < 1e-6, "q_dem={q_dem:.4} != 0.62");
    }

    #[test]
    fn forecast_demand_kw_clamped_at_zero_when_ambient_above_tank() {
        // If ambient > T_mid, loss is negative; result must not go negative.
        let heater = hot_water_heater(); // draw=0.5, k_loss=0.003, T_mid=60
        let q_dem = heater.forecast_demand_kw(80.0); // ambient well above T_mid
        // draw 0.5 + 0.003×(60-80) = 0.5 - 0.06 = 0.44 → positive; still ≥ 0
        assert!(q_dem >= 0.0, "q_dem must be non-negative, got {q_dem}");
    }

    #[test]
    #[ignore = "implemented in Step 4"]
    fn heater_milp_context_declares_e_tank_s_low_sw() {
        // declare_vars() must produce e_tank, s_low, sw vectors each of length n.
        todo!("implement after HeaterMilpContext redesign")
    }

    #[test]
    #[ignore = "implemented in Step 4"]
    fn heater_milp_sw0_fixed_at_zero() {
        // sw[0] must be declared with min=0.0 max=0.0 (no switching at t=0).
        todo!("implement after HeaterMilpContext redesign")
    }

    #[test]
    #[ignore = "implemented in Step 4"]
    fn heater_milp_must_not_run_all_vars_zero() {
        // MustNotRun: z_heat_mid, z_heat_full fixed at 0; e_tank, s_low, sw ≥ 0.
        todo!("implement after HeaterMilpContext redesign")
    }

    #[test]
    #[ignore = "implemented in Step 4"]
    fn heater_milp_constraints_initial_energy_pin() {
        // constraints() must include two inequalities pinning e_tank[0] = e_init_kwh.
        todo!("implement after HeaterMilpContext redesign")
    }

    #[test]
    #[ignore = "implemented in Step 4"]
    fn heater_milp_constraints_dynamics_count() {
        // For n=4: C2 contributes 2×3 = 6 dynamics inequalities.
        todo!("implement after HeaterMilpContext redesign")
    }

    #[test]
    #[ignore = "implemented in Step 4"]
    fn heater_milp_constraints_upper_bound() {
        // C3: n upper-bound constraints e_tank[t] ≤ e_max_kwh.
        todo!("implement after HeaterMilpContext redesign")
    }

    #[test]
    #[ignore = "implemented in Step 4"]
    fn heater_milp_constraints_soft_low() {
        // C4: n soft-lower constraints e_tank[t] + s_low[t] ≥ 0.
        todo!("implement after HeaterMilpContext redesign")
    }

    #[test]
    #[ignore = "implemented in Step 4"]
    fn heater_milp_constraints_switching_four_per_step() {
        // C5: 4×(n−1) switching constraints for n > 1.
        todo!("implement after HeaterMilpContext redesign")
    }
}
