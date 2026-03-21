use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::common::{Interpolation, Quantity, QuantityTimeline, Unit};
use crate::controller::trace::AssetHistoryBuffer;
use crate::profile::HeaterConfig;
use super::{AssetCapabilities, ControlDescriptor, ControlKind, TickEnvironment};

/// Heater: consumes power for space heating (positive = import).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Heater {
    pub temp_c: f64,
    pub max_kw: f64,
    pub temp_min_c: f64,
    pub temp_max_c: f64,
    pub current_kw: f64,
    pub ambient_temp_c: f64,
    thermal_mass: f64, // kWh per degree C
}

impl Heater {
    pub fn from_config(cfg: &HeaterConfig) -> Self {
        Self {
            temp_c: cfg.temp_initial_c,
            max_kw: cfg.max_kw,
            temp_min_c: cfg.temp_min_c,
            temp_max_c: cfg.temp_max_c,
            current_kw: 0.0,
            ambient_temp_c: 10.0,
            thermal_mass: 2.0,
        }
    }

    pub fn update(&mut self, dt_s: f64, setpoint: f64, env: &TickEnvironment) -> f64 {
        if let Some(&amb) = env.get("ambient_temp_c") {
            self.ambient_temp_c = amb;
        }
        let dt_h = dt_s / 3600.0;
        let loss_rate_kw = (self.temp_c - self.ambient_temp_c) * 0.1;
        let heat_loss_kwh = loss_rate_kw * dt_h;
        let kw = setpoint.clamp(0.0, self.max_kw);
        let heat_input_kwh = kw * dt_h;
        let net_energy_kwh = heat_input_kwh - heat_loss_kwh;
        self.temp_c += net_energy_kwh / self.thermal_mass;
        if self.temp_c < self.temp_min_c {
            let forced_kw = self.max_kw;
            self.current_kw = forced_kw;
            return forced_kw;
        }
        if self.temp_c > self.temp_max_c {
            self.current_kw = 0.0;
            return 0.0;
        }
        self.current_kw = kw;
        kw
    }

    pub fn forecast(&self, timespan: Duration) -> QuantityTimeline {
        if timespan <= Duration::zero() {
            return QuantityTimeline::empty(Quantity::Power, Unit::Kilowatt, Interpolation::Linear);
        }
        let now = Utc::now();
        let end = now + timespan;
        let setpoint = self.current_kw.clamp(0.0, self.max_kw);
        let mut samples: Vec<(chrono::DateTime<Utc>, f64)> = Vec::new();

        let mut t = now;
        let mut temp = self.temp_c;

        while t < end {
            // Apply thermal model for this minute step.
            let dt_h = 1.0 / 60.0;
            let loss_rate_kw = (temp - self.ambient_temp_c) * 0.1;
            let heat_loss_kwh = loss_rate_kw * dt_h;
            let kw = if temp < self.temp_min_c {
                self.max_kw // thermostat forces full heat
            } else if temp > self.temp_max_c {
                0.0 // thermostat shuts off
            } else {
                setpoint
            };
            samples.push((t, kw));

            let heat_input_kwh = kw * dt_h;
            let net_energy_kwh = heat_input_kwh - heat_loss_kwh;
            temp += net_energy_kwh / self.thermal_mass;

            t = t + Duration::seconds(60);
        }
        // Mandatory boundary point.
        let end_kw = if temp < self.temp_min_c {
            self.max_kw
        } else if temp > self.temp_max_c {
            0.0
        } else {
            setpoint
        };
        samples.push((end, end_kw));

        QuantityTimeline {
            samples,
            quantity: Quantity::Power,
            unit: Unit::Kilowatt,
            interpolation: Interpolation::Linear,
        }
    }

    pub fn history(&self, timespan: Duration, history: &AssetHistoryBuffer) -> QuantityTimeline {
        super::history_from_buffer(timespan, history, Quantity::Power, Unit::Kilowatt, Interpolation::Linear)
    }

    pub fn state_values(&self) -> HashMap<String, f64> {
        let mut m = HashMap::new();
        m.insert("temp_c".into(), self.temp_c);
        m.insert("max_kw".into(), self.max_kw);
        m.insert("temp_min_c".into(), self.temp_min_c);
        m.insert("temp_max_c".into(), self.temp_max_c);
        m
    }

    pub fn default_setpoint(&self) -> f64 {
        self.max_kw * 0.5
    }

    pub fn capabilities(&self, asset_id: &str) -> AssetCapabilities {
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
        vec![
            ControlDescriptor {
                key: "heater_max_kw".into(),
                label: "Max Heating Power".into(),
                kind: ControlKind::NumberInput,
                min: Some(0.0),
                max: Some(20.0),
                unit: "kW".into(),
            },
            ControlDescriptor {
                key: "heater_temp_min_c".into(),
                label: "Min Temperature".into(),
                kind: ControlKind::Slider,
                min: Some(10.0),
                max: Some(25.0),
                unit: "°C".into(),
            },
            ControlDescriptor {
                key: "heater_temp_max_c".into(),
                label: "Max Temperature".into(),
                kind: ControlKind::Slider,
                min: Some(10.0),
                max: Some(30.0),
                unit: "°C".into(),
            },
        ]
    }

    pub fn reset(&mut self, values: HashMap<String, f64>) {
        if let Some(&t) = values.get("temp_c") {
            self.temp_c = t;
        }
    }

    pub fn update_config(&mut self, values: HashMap<String, f64>) {
        if let Some(&v) = values.get("max_kw") {
            self.max_kw = v.max(0.0);
        }
    }

    pub fn default_comfort_rates(&self) -> Vec<crate::entities::asset::ComfortRate> {
        vec![
            crate::entities::asset::ComfortRate { fill: 0.0, max_marginal_price: 0.30, max_marginal_co2: 0.0 },
            crate::entities::asset::ComfortRate { fill: 1.0, max_marginal_price: 0.10, max_marginal_co2: 0.0 },
        ]
    }

    pub fn default_completion_policy(&self) -> crate::entities::asset::CompletionPolicy {
        crate::entities::asset::CompletionPolicy::Continue
    }

    pub fn default_post_deadline_comfort_bid(&self) -> Option<f64> {
        Some(0.10)
    }

    pub fn power_w(&self) -> f64 {
        self.current_kw * 1000.0
    }
}
