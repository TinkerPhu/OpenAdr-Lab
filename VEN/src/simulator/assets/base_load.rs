use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::common::{Interpolation, Quantity, QuantitySeries, Unit};
use crate::controller::trace::AssetHistoryBuffer;
use crate::profile::BaseLoadConfig;
use super::{AssetCapabilities, ControlDescriptor, TickEnvironment};

/// Base load: fixed background consumption (positive = import).
/// Non-flexible — planner never schedules allocations for this asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseLoad {
    pub baseline_kw: f64,
    pub current_kw: f64,
}

impl BaseLoad {
    pub fn from_config(cfg: &BaseLoadConfig) -> Self {
        Self {
            baseline_kw: cfg.baseline_kw,
            current_kw: cfg.baseline_kw,
        }
    }

    pub fn update(&mut self, _dt_s: f64, _setpoint: f64, _env: &TickEnvironment) -> f64 {
        self.current_kw = self.baseline_kw;
        self.baseline_kw
    }

    pub fn forecast(&self, _timespan: Duration) -> QuantitySeries {
        QuantitySeries::empty(Quantity::Power, Unit::Kilowatt, Interpolation::Step)
    }

    pub fn past(&self, _timespan: Duration, _history: &AssetHistoryBuffer) -> QuantitySeries {
        QuantitySeries::empty(Quantity::Power, Unit::Kilowatt, Interpolation::Step)
    }

    pub fn state_values(&self) -> HashMap<String, f64> {
        HashMap::new()
    }

    pub fn default_setpoint(&self) -> f64 {
        self.baseline_kw
    }

    pub fn capabilities(&self, asset_id: &str) -> AssetCapabilities {
        AssetCapabilities {
            asset_id: asset_id.to_string(),
            max_import_kw: self.baseline_kw,
            max_export_kw: 0.0,
            is_flexible: false,
            energy_state: None,
            availability: None,
        }
    }

    pub fn control_schema(&self) -> Vec<ControlDescriptor> {
        vec![] // non-flexible, no runtime controls
    }

    pub fn reset(&mut self, _values: HashMap<String, f64>) {
        // nothing to reset
    }

    pub fn update_config(&mut self, values: HashMap<String, f64>) {
        if let Some(&v) = values.get("baseline_kw") {
            self.baseline_kw = v.max(0.0);
            self.current_kw = self.baseline_kw;
        }
    }
}
