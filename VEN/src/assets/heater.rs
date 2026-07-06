use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{Asset, AssetCapability, AssetState, ControlDescriptor, ControlKind};
use crate::common::{Interpolation, TimeSeries};
use crate::entities::asset_params::HeaterParams;
use crate::entities::timeline::HeaterPlanTrajectory;

/// Heater config. Consumes power for space heating (positive = import).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Heater {
    pub max_kw: f64,
    /// Mid-tier power level [kW]. Hardware relay step: 0 / mid_kw / max_kw are the only
    /// valid states. Setpoints are quantized to the nearest tier in step_inner().
    /// Default 0.0 means "use max_kw / 2.0" — handles old persisted JSON without this field.
    #[serde(default)]
    pub mid_kw: f64,
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
    pub fn from_params(cfg: &HeaterParams) -> Self {
        Self {
            max_kw: cfg.max_kw,
            mid_kw: cfg.mid_kw.unwrap_or(cfg.max_kw / 2.0),
            min_power_kw: 0.0,
            temp_min_c: cfg.temp_min_c,
            temp_max_c: cfg.temp_max_c,
            temp_min_c_profile: cfg.temp_min_c,
            temp_max_c_profile: cfg.temp_max_c,
            thermal_mass_kwh_per_c: cfg.thermal_mass_kwh_per_c,
            k_loss_kw_per_c: cfg.k_loss_kw_per_c,
            draw_kw: cfg.draw_kw,
            ambient_temp_c: 10.0,
        }
    }

    pub fn initial_state(cfg: &HeaterParams) -> HeaterState {
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
        // Quantize to the nearest valid hardware tier: 0 / mid_kw / max_kw.
        // The heater has two physical relays; intermediate values are impossible.
        // mid_kw = 0.0 means "not set" (old persisted JSON); fall back to max_kw / 2.0.
        let mid = if self.mid_kw > 0.0 {
            self.mid_kw
        } else {
            self.max_kw / 2.0
        };
        let tier = if setpoint_kw < mid / 2.0 {
            0.0
        } else if setpoint_kw < (mid + self.max_kw) / 2.0 {
            mid
        } else {
            self.max_kw
        };
        // Thermostat overrides with hysteresis: once emergency fires at T_min,
        // keep running until T_min + 3 °C to prevent rapid relay cycling.
        // actual_power_kw from the previous tick is the implicit thermostat state.
        const EMERGENCY_HYSTERESIS_C: f64 = 3.0;
        let emergency_active = state.temperature_c <= self.temp_min_c
            || (state.actual_power_kw >= self.max_kw
                && state.temperature_c < self.temp_min_c + EMERGENCY_HYSTERESIS_C);
        let actual = if state.temperature_c >= self.temp_max_c {
            0.0
        } else if emergency_active {
            self.max_kw
        } else {
            tier
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
        // Off between plan slots; thermostat emergency and plan allocations turn it on.
        0.0
    }

    pub fn state_values(&self, state: &HeaterState) -> HashMap<String, f64> {
        let mut m = HashMap::new();
        m.insert("temp_c".into(), state.temperature_c);
        m.insert("max_kw".into(), self.max_kw);
        m.insert("mid_kw".into(), self.mid_kw);
        m.insert("temp_min_c".into(), self.temp_min_c);
        m.insert("temp_max_c".into(), self.temp_max_c);
        m
    }

    /// State values for a future MILP time slot, given the thermal energy stored
    /// above `temp_min_c` at the start of that slot (kWh).
    /// Returns `{"temp_c": <temperature>}`.
    pub fn future_state_values(&self, e_tank_kwh: f64) -> HashMap<String, f64> {
        let temp_c = self.temp_min_c + e_tank_kwh / self.thermal_mass_kwh_per_c;
        HashMap::from([("temp_c".into(), temp_c)])
    }

    /// Create a plan trajectory starting from the current live state.
    /// Returns `None` if `live_state` is not a heater state.
    pub fn plan_trajectory(
        cfg: &Self,
        live_state: &super::AssetState,
    ) -> Option<HeaterPlanTrajectory> {
        if let super::AssetState::Heater(s) = live_state {
            let e_max_kwh = (cfg.temp_max_c - cfg.temp_min_c) * cfg.thermal_mass_kwh_per_c;
            let e_kwh = ((s.temperature_c - cfg.temp_min_c) * cfg.thermal_mass_kwh_per_c)
                .clamp(0.0, e_max_kwh);
            Some(HeaterPlanTrajectory {
                e_kwh,
                temp_min_c: cfg.temp_min_c,
                thermal_mass: cfg.thermal_mass_kwh_per_c,
                q_dem_kw: cfg.forecast_demand_kw(cfg.ambient_temp_c),
                e_max_kwh,
            })
        } else {
            None
        }
    }

    pub fn control_schema(&self) -> Vec<ControlDescriptor> {
        vec![
            ControlDescriptor {
                key: "heater_temp_c".into(),
                label: "T_tank".into(),
                kind: ControlKind::Slider,
                min: Some(18.0),
                max: Some(95.0),
                unit: "°C".into(),
                display_scale: None,
            },
            ControlDescriptor {
                key: "heater_setpoint_c".into(),
                label: "Power setpoint".into(),
                kind: ControlKind::Slider,
                min: Some(0.0),
                max: Some(self.max_kw),
                unit: "kW".into(),
                display_scale: None,
            },
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
        // Simulate uncontrolled thermostat operation (no plan overlay, setpoint = 0).
        // The thermostat emergency fires when temp ≤ T_min, so the forecast still
        // captures long-run thermal cycling rather than a flat-zero line.
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
            t += Duration::seconds(60);
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

    /// Constant per-step thermal demand forecast [kW].
    /// Uses the midpoint of the comfort band as the representative tank temperature.
    /// `Q_dem = draw_kw + k_loss × (T_mid − ambient_temp_c)`
    pub fn forecast_demand_kw(&self, ambient_temp_c: f64) -> f64 {
        let t_mid = (self.temp_min_c + self.temp_max_c) / 2.0;
        (self.draw_kw + self.k_loss_kw_per_c * (t_mid - ambient_temp_c)).max(0.0)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn default_heater() -> Heater {
        Heater {
            max_kw: 2.5,
            mid_kw: 1.25,
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
            max_kw: 6.0,
            mid_kw: 3.0,
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
    fn control_schema_returns_four_descriptors() {
        let heater = default_heater();
        let schema = heater.control_schema();
        let keys: Vec<_> = schema.iter().map(|d| d.key.as_str()).collect();
        assert!(keys.contains(&"heater_temp_c"), "missing heater_temp_c");
        assert!(
            keys.contains(&"heater_setpoint_c"),
            "missing heater_setpoint_c"
        );
        assert!(
            keys.contains(&"heater_temp_min_c"),
            "missing heater_temp_min_c"
        );
        assert!(
            keys.contains(&"heater_temp_max_c"),
            "missing heater_temp_max_c"
        );
        assert_eq!(schema.len(), 4, "expected exactly 4 control descriptors");
    }

    #[test]
    fn control_schema_heater_setpoint_bounds() {
        let heater = default_heater();
        let schema = heater.control_schema();
        let sp_d = schema
            .iter()
            .find(|d| d.key == "heater_setpoint_c")
            .unwrap();
        let temp_d = schema.iter().find(|d| d.key == "heater_temp_c").unwrap();
        assert_eq!(sp_d.min.unwrap(), 0.0);
        assert_eq!(sp_d.max.unwrap(), heater.max_kw);
        assert_eq!(temp_d.min.unwrap(), 18.0);
        assert_eq!(temp_d.max.unwrap(), 95.0);
    }

    #[test]
    fn control_schema_t_tank_bounds_are_18_to_95() {
        let heater = default_heater();
        let schema = heater.control_schema();
        let min_d = schema
            .iter()
            .find(|d| d.key == "heater_temp_min_c")
            .unwrap();
        let max_d = schema
            .iter()
            .find(|d| d.key == "heater_temp_max_c")
            .unwrap();
        assert_eq!(min_d.min.unwrap(), 18.0);
        assert_eq!(min_d.max.unwrap(), 94.0);
        assert_eq!(max_d.min.unwrap(), 19.0);
        assert_eq!(max_d.max.unwrap(), 95.0);
        assert_eq!(min_d.label, "T_tank_min");
        assert_eq!(max_d.label, "T_tank_max");
    }

    // ── forecast ─────────────────────────────────────────────────────────────

    /// When the heater is at temp_max (thermostat forced off), the forecast simulates
    /// thermostat-only operation (setpoint=0). The tank cools to T_min, the emergency
    /// fires at max_kw, and the cycle repeats — average power ≈ heat loss at T_min.
    #[test]
    fn forecast_at_temp_max_gives_non_zero_average_power() {
        let heater = default_heater();
        let state = state_at(23.0, 0.0);
        // thermal_mass=2.0 kWh/°C → τ=20h; T drops from 23→20°C in ~5h.
        // Use 24h to ensure full thermostat cycling is captured.
        let ts = heater.forecast(&state, Duration::hours(24));

        // Compute mean power over the forecast samples
        let n = ts.samples.len() as f64;
        assert!(n > 0.0, "forecast produced no samples");
        let mean: f64 = ts.samples.iter().map(|(_, kw)| kw).sum::<f64>() / n;

        // Thermostat cycles near T_min=20°C → heat loss ≈ 0.1×(20-10) = 1.0 kW.
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
        // thermal_mass=2.0 kWh/°C → T drops from 21.5→20°C (T_min) in ~2.8h.
        // Use 12h to ensure cycling is captured in the mean.
        let ts = heater.forecast(&state, Duration::hours(12));
        let n = ts.samples.len() as f64;
        assert!(n > 0.0);
        let mean: f64 = ts.samples.iter().map(|(_, kw)| kw).sum::<f64>() / n;
        // Expect long-run equilibrium in reasonable range
        assert!(
            (0.5..=2.5).contains(&mean),
            "mean {mean:.3} kW out of range"
        );
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
        assert_eq!(
            power, heater.max_kw,
            "heater must run at max_kw below temp_min"
        );
    }

    #[test]
    fn heater_follows_setpoint_in_comfort_band() {
        let heater = default_heater();
        let state = state_at(21.5, 0.0);
        let setpoint = 1.5;
        let (_ns, power) = heater.step_inner(&state, setpoint, Duration::seconds(1));
        // Relay quantization: 1.5 kW falls between mid/2=0.625 and (mid+max)/2=1.875,
        // so it snaps to the mid tier (1.25 kW). Exact passthrough is no longer possible.
        assert!(
            (power - heater.mid_kw).abs() < 1e-9,
            "heater should snap setpoint 1.5 to mid tier {}, got {power}",
            heater.mid_kw
        );
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
            actual_delta,
            expected_delta
        );
    }

    #[test]
    fn hwt_draw_drains_tank_when_off() {
        // With 0.5 kW draw and no heater, tank should cool faster than without draw.
        let heater = hot_water_heater();
        let no_draw = Heater {
            draw_kw: 0.0,
            ..hot_water_heater()
        };
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
        assert_eq!(
            power, heater.max_kw,
            "emergency: must run at max below temp_min"
        );
    }

    #[test]
    fn hwt_forced_off_above_temp_max() {
        let heater = hot_water_heater();
        let state = state_at(80.1, 3.0);
        let (_ns, power) = heater.step_inner(&state, 3.0, Duration::seconds(1));
        assert_eq!(power, 0.0, "must be forced off above temp_max");
    }

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

    // T016: Heater::future_state_values returns correct temp_c.
    #[test]
    fn future_state_values_mid_energy() {
        let h = default_heater(); // thermal_mass_kwh_per_c = 2.0, temp_min_c = 20.0
                                  // 2.0 kWh stored → temp = 20.0 + 2.0 / 2.0 = 21.0 °C
        let vals = h.future_state_values(2.0);
        let temp_c = vals["temp_c"];
        assert!(
            (temp_c - 21.0).abs() < 1e-9,
            "expected temp_c=21.0, got {temp_c}"
        );
    }

    #[test]
    fn future_state_values_zero_energy() {
        let h = default_heater();
        let vals = h.future_state_values(0.0);
        assert!((vals["temp_c"] - h.temp_min_c).abs() < 1e-9);
    }

    #[test]
    fn future_state_values_returns_only_temp_c() {
        let h = default_heater();
        let vals = h.future_state_values(1.0);
        assert_eq!(vals.len(), 1, "expected exactly one key");
        assert!(vals.contains_key("temp_c"));
    }
}

#[cfg(test)]
mod param_tests {
    use super::*;

    #[test]
    fn heater_params_defaults() {
        let params = HeaterParams::default();
        assert!((params.max_kw - 5.0).abs() < f64::EPSILON);
        assert_eq!(params.mid_kw, None);
    }

    #[test]
    fn heater_params_mid_kw_some_none() {
        assert_eq!(HeaterParams::default().mid_kw, None);
        let params = HeaterParams {
            mid_kw: Some(2.5),
            ..HeaterParams::default()
        };
        assert_eq!(params.mid_kw, Some(2.5));
    }
}
