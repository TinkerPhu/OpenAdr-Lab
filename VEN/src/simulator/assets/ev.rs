use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::profile::EvConfig;
use super::{AssetCapabilities, ControlDescriptor, ControlKind, EnergyState, TickEnvironment};

/// EV Charger: consumes power to charge battery (positive = import).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvCharger {
    pub soc: f64,
    pub plugged: bool,
    pub max_charge_kw: f64,
    pub max_discharge_kw: f64,
    pub battery_kwh: f64,
    pub soc_target: f64,
    pub default_charge_kw: f64,
    pub current_kw: f64,
}

impl EvCharger {
    pub fn from_config(cfg: &EvConfig) -> Self {
        Self {
            soc: cfg.initial_soc,
            plugged: true,
            max_charge_kw: cfg.max_charge_kw,
            max_discharge_kw: cfg.max_discharge_kw,
            battery_kwh: cfg.battery_kwh,
            soc_target: cfg.soc_target.max(cfg.initial_soc),
            default_charge_kw: cfg.default_charge_kw,
            current_kw: cfg.max_charge_kw,
        }
    }

    pub fn update(&mut self, dt_s: f64, setpoint: f64, _env: &TickEnvironment) -> f64 {
        if !self.plugged {
            self.current_kw = 0.0;
            return 0.0;
        }
        let kw = setpoint.clamp(-self.max_discharge_kw, self.max_charge_kw);
        let kw = if kw > 0.0 && self.soc >= 1.0 {
            0.0
        } else if kw < 0.0 && self.soc <= 0.0 {
            0.0
        } else {
            kw
        };
        let dt_h = dt_s / 3600.0;
        self.soc += (kw * dt_h) / self.battery_kwh;
        self.soc = self.soc.clamp(0.0, 1.0);
        self.current_kw = kw;
        kw
    }

    pub fn predict(&self, setpoint: f64) -> Vec<(chrono::DateTime<Utc>, f64)> {
        vec![(Utc::now(), setpoint)]
    }

    pub fn state_values(&self) -> HashMap<String, f64> {
        let mut m = HashMap::new();
        m.insert("soc_pct".into(), self.soc * 100.0);
        m.insert("plugged".into(), if self.plugged { 1.0 } else { 0.0 });
        m.insert("current_kw".into(), self.current_kw);
        m
    }

    pub fn default_setpoint(&self) -> f64 {
        self.default_charge_kw
    }

    pub fn capabilities(&self, asset_id: &str) -> AssetCapabilities {
        AssetCapabilities {
            asset_id: asset_id.to_string(),
            max_import_kw: self.max_charge_kw,
            max_export_kw: self.max_discharge_kw,
            is_flexible: true,
            energy_state: Some(EnergyState {
                current_kwh: self.soc * self.battery_kwh,
                min_kwh: 0.0,
                max_kwh: self.battery_kwh,
            }),
            availability: None,
        }
    }

    pub fn control_schema(&self) -> Vec<ControlDescriptor> {
        vec![
            ControlDescriptor {
                key: "ev_desired_kw".into(),
                label: "Charge Rate".into(),
                kind: ControlKind::Slider,
                min: Some(0.0),
                max: Some(self.max_charge_kw),
                unit: "kW".into(),
            },
            ControlDescriptor {
                key: "ev_plugged".into(),
                label: "Plugged In".into(),
                kind: ControlKind::Switch,
                min: None,
                max: None,
                unit: "".into(),
            },
            ControlDescriptor {
                key: "ev_soc_target".into(),
                label: "SoC Target".into(),
                kind: ControlKind::Slider,
                min: Some(0.0),
                max: Some(1.0),
                unit: "%".into(),
            },
        ]
    }

    pub fn reset(&mut self, values: HashMap<String, f64>) {
        if let Some(&soc) = values.get("soc") {
            self.soc = soc.clamp(0.0, 1.0);
        }
    }

    pub fn update_config(&mut self, values: HashMap<String, f64>) {
        if let Some(&v) = values.get("max_charge_kw") {
            self.max_charge_kw = v.max(0.0);
        }
    }

    pub fn power_w(&self) -> f64 {
        self.current_kw * 1000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_env() -> TickEnvironment { HashMap::new() }

    #[test]
    fn ev_charges_and_stops_at_full() {
        let mut ev = EvCharger {
            soc: 0.99, plugged: true, max_charge_kw: 10.0, max_discharge_kw: 0.0,
            battery_kwh: 10.0, soc_target: 1.0, default_charge_kw: 0.0, current_kw: 0.0,
        };
        for _ in 0..1000 { ev.update(1.0, 10.0, &make_env()); }
        assert!((ev.soc - 1.0).abs() < 0.001);
        assert_eq!(ev.update(1.0, 10.0, &make_env()), 0.0);
    }

    #[test]
    fn ev_discharges_v2g_and_stops_at_empty() {
        let mut ev = EvCharger {
            soc: 0.01, plugged: true, max_charge_kw: 10.0, max_discharge_kw: 10.0,
            battery_kwh: 10.0, soc_target: 1.0, default_charge_kw: 0.0, current_kw: 0.0,
        };
        for _ in 0..1000 { ev.update(1.0, -10.0, &make_env()); }
        assert!((ev.soc - 0.0).abs() < 0.001);
        assert_eq!(ev.update(1.0, -10.0, &make_env()), 0.0);
    }
}
