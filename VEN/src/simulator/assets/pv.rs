use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::profile::PvConfig;
use super::{AssetCapabilities, ControlDescriptor, ControlKind, TickEnvironment};

/// PV Inverter: generates power.
/// `current_kw` stores the positive generation value (internal).
/// Sign convention (export = negative) is applied via `power_kw` in the API response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PvInverter {
    pub rated_kw: f64,
    pub irradiance: f64,              // 0.0..1.0
    pub export_limit_kw: Option<f64>, // active cap; None = no limit
    pub current_kw: f64,              // generation magnitude (positive)
}

impl PvInverter {
    pub fn from_config(cfg: &PvConfig) -> Self {
        Self {
            rated_kw: cfg.rated_kw,
            irradiance: 0.0,
            export_limit_kw: None,
            current_kw: 0.0,
        }
    }

    /// `setpoint` encodes export limit: f64::MAX (or very large) = no limit, else kW cap.
    /// Reads "hour_of_day" and "pv_irradiance" from env.
    pub fn update(&mut self, _dt_s: f64, setpoint: f64, env: &TickEnvironment) -> f64 {
        let hour = env.get("hour_of_day").copied().unwrap_or(12.0);
        let irradiance_override = env.get("pv_irradiance").copied();

        self.irradiance = irradiance_override.unwrap_or_else(|| {
            if hour >= 6.0 && hour <= 18.0 {
                let angle = std::f64::consts::PI * (hour - 6.0) / 12.0;
                angle.sin()
            } else {
                0.0
            }
        });

        let natural_output = self.rated_kw * self.irradiance;
        let limit_kw = if setpoint >= f64::MAX * 0.5 {
            None
        } else {
            Some(setpoint.max(0.0))
        };
        let output = match limit_kw {
            Some(limit) => natural_output.min(limit),
            None => natural_output,
        };
        self.export_limit_kw = limit_kw;
        self.current_kw = output;
        output
    }

    pub fn predict(&self, setpoint: f64) -> Vec<(chrono::DateTime<Utc>, f64)> {
        let limit = if setpoint >= f64::MAX * 0.5 { self.rated_kw } else { setpoint.max(0.0) };
        vec![(Utc::now(), (self.rated_kw * self.irradiance).min(limit))]
    }

    pub fn state_values(&self) -> HashMap<String, f64> {
        let mut m = HashMap::new();
        m.insert("irradiance".into(), self.irradiance);
        m.insert("rated_kw".into(), self.rated_kw);
        if let Some(lim) = self.export_limit_kw {
            m.insert("export_limit_kw".into(), lim);
        }
        m
    }

    pub fn default_setpoint(&self) -> f64 {
        f64::MAX // no export limit by default
    }

    pub fn capabilities(&self, asset_id: &str) -> AssetCapabilities {
        AssetCapabilities {
            asset_id: asset_id.to_string(),
            max_import_kw: 0.0,
            max_export_kw: self.rated_kw,
            is_flexible: false,
            energy_state: None,
            availability: None,
        }
    }

    pub fn control_schema(&self) -> Vec<ControlDescriptor> {
        vec![
            ControlDescriptor {
                key: "pv_irradiance".into(),
                label: "Irradiance Override".into(),
                kind: ControlKind::Slider,
                min: Some(0.0),
                max: Some(1.0),
                unit: "".into(),
            },
            ControlDescriptor {
                key: "pv_force_export_limit_kw".into(),
                label: "Export Limit".into(),
                kind: ControlKind::NumberInput,
                min: Some(0.0),
                max: Some(self.rated_kw * 2.0),
                unit: "kW".into(),
            },
        ]
    }

    pub fn reset(&mut self, _values: HashMap<String, f64>) {}

    pub fn update_config(&mut self, values: HashMap<String, f64>) {
        if let Some(&v) = values.get("rated_kw") {
            self.rated_kw = v.max(0.0);
        }
    }

    pub fn generation_w(&self) -> f64 {
        self.current_kw * 1000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_hour(h: f64) -> TickEnvironment {
        let mut e = HashMap::new();
        e.insert("hour_of_day".into(), h);
        e
    }

    fn env_irradiance(v: f64) -> TickEnvironment {
        let mut e = HashMap::new();
        e.insert("pv_irradiance".into(), v);
        e
    }

    #[test]
    fn pv_sinusoidal_model() {
        let cfg = PvConfig { id: "pv".to_string(), rated_kw: 10.0 };
        let mut pv = PvInverter::from_config(&cfg);
        let kw = pv.update(1.0, f64::MAX, &env_hour(12.0));
        assert!((kw - 10.0).abs() < 0.01, "noon should be ~10kW, got {kw}");
        let kw = pv.update(1.0, f64::MAX, &env_hour(0.0));
        assert_eq!(kw, 0.0);
        let kw = pv.update(1.0, 0.0, &env_hour(12.0));
        assert_eq!(kw, 0.0);
        let kw = pv.update(1.0, 5.0, &env_hour(12.0));
        assert!((kw - 5.0).abs() < 0.01);
        let kw = pv.update(1.0, f64::MAX, &env_irradiance(1.0));
        assert!((kw - 10.0).abs() < 0.01);
    }
}
