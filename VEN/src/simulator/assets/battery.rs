use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::profile::BatteryConfig;
use super::{AssetCapabilities, ControlDescriptor, ControlKind, EnergyState, TickEnvironment};

/// Battery storage: bidirectional.
/// Positive = charge (import), negative = discharge (export).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Battery {
    pub soc: f64,
    pub capacity_kwh: f64,
    pub max_charge_kw: f64,
    pub max_discharge_kw: f64,
    pub round_trip_efficiency: f64,
    pub min_soc: f64,
    pub current_kw: f64,
}

impl Battery {
    pub fn from_config(cfg: &BatteryConfig) -> Self {
        Self {
            soc: cfg.initial_soc,
            capacity_kwh: cfg.capacity_kwh,
            max_charge_kw: cfg.max_charge_kw,
            max_discharge_kw: cfg.max_discharge_kw,
            round_trip_efficiency: cfg.round_trip_efficiency,
            min_soc: cfg.min_soc,
            current_kw: 0.0,
        }
    }

    pub fn update(&mut self, dt_s: f64, setpoint: f64, _env: &TickEnvironment) -> f64 {
        let kw = setpoint.clamp(-self.max_discharge_kw, self.max_charge_kw);
        let kw = if kw > 0.0 && self.soc >= 1.0 {
            0.0
        } else if kw < 0.0 && self.soc <= self.min_soc {
            0.0
        } else {
            kw
        };
        let dt_h = dt_s / 3600.0;
        if kw > 0.0 {
            self.soc += (kw * dt_h * self.round_trip_efficiency) / self.capacity_kwh;
        } else {
            self.soc += (kw * dt_h) / self.capacity_kwh;
        }
        self.soc = self.soc.clamp(0.0, 1.0);
        self.current_kw = kw;
        kw
    }

    pub fn predict(&self, setpoint: f64) -> Vec<(chrono::DateTime<Utc>, f64)> {
        vec![(Utc::now(), setpoint.clamp(-self.max_discharge_kw, self.max_charge_kw))]
    }

    pub fn state_values(&self) -> HashMap<String, f64> {
        let mut m = HashMap::new();
        m.insert("soc_pct".into(), self.soc * 100.0);
        m.insert("soc".into(), self.soc);
        m.insert("current_kw".into(), self.current_kw);
        m.insert("capacity_kwh".into(), self.capacity_kwh);
        m.insert("max_charge_kw".into(), self.max_charge_kw);
        m.insert("max_discharge_kw".into(), self.max_discharge_kw);
        m.insert("min_soc".into(), self.min_soc);
        m
    }

    pub fn default_setpoint(&self) -> f64 {
        0.0 // hold by default; dispatcher controls
    }

    pub fn capabilities(&self, asset_id: &str) -> AssetCapabilities {
        AssetCapabilities {
            asset_id: asset_id.to_string(),
            max_import_kw: self.max_charge_kw,
            max_export_kw: self.max_discharge_kw,
            is_flexible: true,
            energy_state: Some(EnergyState {
                current_kwh: self.soc * self.capacity_kwh,
                min_kwh: self.min_soc * self.capacity_kwh,
                max_kwh: self.capacity_kwh,
            }),
            availability: None,
        }
    }

    pub fn control_schema(&self) -> Vec<ControlDescriptor> {
        vec![ControlDescriptor {
            key: "battery_force_kw".into(),
            label: "Force Power".into(),
            kind: ControlKind::Slider,
            min: Some(-self.max_discharge_kw),
            max: Some(self.max_charge_kw),
            unit: "kW".into(),
        }]
    }

    pub fn reset(&mut self, values: HashMap<String, f64>) {
        if let Some(&soc) = values.get("soc") {
            self.soc = soc.clamp(0.0, 1.0);
        }
    }

    pub fn update_config(&mut self, values: HashMap<String, f64>) {
        if let Some(&v) = values.get("capacity_kwh") {
            self.capacity_kwh = v.max(0.1);
        }
    }

    pub fn power_w(&self) -> f64 {
        self.current_kw * 1000.0
    }

    /// Resolve a user request target for this battery.
    /// Returns (target_energy_kwh, desired_power_kw), or None if already at/above target.
    pub fn resolve_request_target(
        &self,
        target_soc: Option<f64>,
        desired_power_kw: Option<f64>,
    ) -> Option<(f64, f64)> {
        let target = target_soc.unwrap_or(1.0);
        let delta = (target - self.soc).max(0.0);
        let kwh = delta * self.capacity_kwh;
        if kwh < 1e-6 {
            return None;
        }
        Some((kwh, desired_power_kw.unwrap_or(self.max_charge_kw)))
    }
}
