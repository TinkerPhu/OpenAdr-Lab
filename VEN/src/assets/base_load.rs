use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{Asset, AssetCapability, AssetState, ControlDescriptor, ControlKind};
use crate::common::{Interpolation, TimeSeries};

#[derive(Debug, Clone)]
pub struct BaseLoadParams {
    pub id: String,
    pub baseline_kw: f64,
}

impl Default for BaseLoadParams {
    fn default() -> Self {
        Self {
            id: crate::ids::ASSET_BASE_LOAD.to_string(),
            baseline_kw: 0.5,
        }
    }
}

/// Base load config. Fixed background consumption (positive = import).
/// Non-flexible — planner never schedules allocations for this asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseLoad {
    pub baseline_kw: f64,
    /// Original profile value — used for snap-back when inject override is released.
    pub baseline_kw_profile: f64,
}

/// BaseLoad mutable state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseLoadState {
    /// Actual power last tick. Always ≥ 0 (consumption only). Unit: kW.
    pub actual_power_kw: f64,
}

impl BaseLoad {
    pub fn from_params(cfg: &BaseLoadParams) -> Self {
        Self {
            baseline_kw: cfg.baseline_kw,
            baseline_kw_profile: cfg.baseline_kw,
        }
    }

    pub fn initial_state(cfg: &BaseLoadParams) -> BaseLoadState {
        BaseLoadState {
            actual_power_kw: cfg.baseline_kw,
        }
    }

    /// Pure physics step. Always returns baseline_kw; setpoint and state are ignored.
    pub fn step_inner(
        &self,
        _state: &BaseLoadState,
        _setpoint_kw: f64,
        _dt: Duration,
    ) -> (BaseLoadState, f64) {
        let actual_kw = self.baseline_kw;
        (
            BaseLoadState {
                actual_power_kw: actual_kw,
            },
            actual_kw,
        )
    }

    /// Point-range capability (non-curtailable).
    pub fn capability_inner(&self, state: &BaseLoadState) -> AssetCapability {
        AssetCapability {
            max_export_kw: state.actual_power_kw,
            max_import_kw: state.actual_power_kw,
        }
    }

    pub fn default_setpoint(&self) -> f64 {
        self.baseline_kw
    }

    pub fn state_values(&self, _state: &BaseLoadState) -> HashMap<String, f64> {
        let mut m = HashMap::new();
        m.insert("baseline_kw".into(), self.baseline_kw);
        m
    }

    pub fn control_schema(&self) -> Vec<ControlDescriptor> {
        vec![
            ControlDescriptor {
                key: "base_load_kw".into(),
                label: "Base Load Override".into(),
                kind: ControlKind::Slider,
                min: Some(0.0),
                max: Some(6.0),
                unit: "kW".into(),
                display_scale: None,
            },
            ControlDescriptor {
                key: "base_load_alpha".into(),
                label: "Blend-back Speed".into(),
                kind: ControlKind::Slider,
                min: Some(0.01),
                max: Some(1.0),
                unit: "".into(),
                display_scale: None,
            },
        ]
    }

    pub fn reset(&self, _state: &mut BaseLoadState, _values: HashMap<String, f64>) {}

    pub fn update_config(&mut self, values: HashMap<String, f64>) {
        if let Some(&v) = values.get("baseline_kw") {
            self.baseline_kw = v.max(0.0);
        }
    }

    pub fn forecast(&self, _state: &BaseLoadState, timespan: Duration) -> TimeSeries {
        if timespan <= Duration::zero() {
            return TimeSeries::empty(Interpolation::Step);
        }
        let now = Utc::now();
        TimeSeries {
            samples: vec![(now, self.baseline_kw), (now + timespan, self.baseline_kw)],
            interpolation: Interpolation::Step,
        }
    }

    pub fn default_comfort_rates(&self) -> Vec<crate::entities::asset::ComfortRate> {
        vec![
            crate::entities::asset::ComfortRate {
                fill: 0.0,
                max_marginal_price: 0.0,
                max_marginal_co2: 0.0,
            },
            crate::entities::asset::ComfortRate {
                fill: 1.0,
                max_marginal_price: 0.0,
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
}

impl Asset for BaseLoad {
    fn step(&self, state: &AssetState, setpoint_kw: f64, dt: Duration) -> (AssetState, f64) {
        let AssetState::BaseLoad(s) = state else {
            unreachable!("BaseLoad/state mismatch")
        };
        let (ns, p) = self.step_inner(s, setpoint_kw, dt);
        (AssetState::BaseLoad(ns), p)
    }

    fn capability(&self, state: &AssetState) -> AssetCapability {
        let AssetState::BaseLoad(s) = state else {
            unreachable!()
        };
        self.capability_inner(s)
    }
}

// ── BaseLoad MILP plugin types ────────────────────────────────────────────────

/// Pre-computed baseline load contribution for one planning cycle.
/// BaseLoad has no LP decision variables — it contributes a constant demand
/// to the power balance at each slot.
#[derive(Debug, Clone)]
pub struct BaseLoadMilpContext {
    /// Background consumption [kW] per slot (positive = import demand).
    pub p_base_kw: Vec<f64>,
}

impl BaseLoad {
    /// Build the BaseLoad MILP context: `n` slots of constant baseline_kw.
    /// Callers may add per-slot `BaselineOverride` adjustments to the returned vec.
    pub fn build_milp_context(&self, n: usize) -> BaseLoadMilpContext {
        BaseLoadMilpContext {
            p_base_kw: vec![self.baseline_kw; n],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_load_params_baseline() {
        let params = BaseLoadParams {
            baseline_kw: 1.5,
            ..BaseLoadParams::default()
        };
        assert!((params.baseline_kw - 1.5).abs() < f64::EPSILON);
    }
}
