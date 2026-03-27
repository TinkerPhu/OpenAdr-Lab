use chrono::{DateTime, Duration, Utc};
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
    /// Active comfort band lower bound. Overridable at runtime via SimInjectState.
    pub temp_min_c: f64,
    /// Active comfort band upper bound. Overridable at runtime via SimInjectState.
    pub temp_max_c: f64,
    /// Original profile value — used for snap-back when inject override is released.
    pub temp_min_c_profile: f64,
    /// Original profile value — used for snap-back when inject override is released.
    pub temp_max_c_profile: f64,
    /// Thermal mass in kWh/°C (hardcoded 2.0 previously — now explicit config).
    pub thermal_mass_kwh_per_c: f64,
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
            thermal_mass_kwh_per_c: 2.0,
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
            clamped.max(self.min_power_kw)
        } else {
            clamped
        };
        // Thermal model: loss = 0.1 kW/°C
        let loss_kw = (state.temperature_c - self.ambient_temp_c) * 0.1;
        let delta_c = (actual - loss_kw) / self.thermal_mass_kwh_per_c * dt_h;
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
        vec![ControlDescriptor {
            key: "heater_setpoint_c".into(),
            label: "Temperature Setpoint".into(),
            kind: ControlKind::Slider,
            min: Some(self.temp_min_c),
            max: Some(self.temp_max_c),
            unit: "°C".into(),
        }]
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
        let setpoint = state.actual_power_kw.clamp(0.0, self.max_kw);
        let mut samples: Vec<(DateTime<Utc>, f64)> = Vec::new();

        let mut t = now;
        let mut temp = state.temperature_c;

        while t < end {
            let dt_h = 1.0 / 60.0;
            let loss_kw = (temp - self.ambient_temp_c) * 0.1;
            let kw = if temp < self.temp_min_c {
                self.max_kw
            } else if temp > self.temp_max_c {
                0.0
            } else {
                setpoint
            };
            samples.push((t, kw));
            let net_kwh = (kw - loss_kw) * dt_h;
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
