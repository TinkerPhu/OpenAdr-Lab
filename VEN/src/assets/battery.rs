use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{
    Asset, AssetCapabilities, AssetCapability, AssetState, ControlDescriptor, ControlKind,
    EnergyState,
};
use crate::common::{Interpolation, TimeSeries};
use crate::controller::trace::AssetHistoryBuffer;
use crate::profile::BatteryConfig;

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
    pub soc_pct: f64,
    /// Actual power last tick. Positive = charging (import). Negative = discharging (export).
    pub actual_power_kw: f64,
}

impl Battery {
    pub fn from_config(cfg: &BatteryConfig) -> Self {
        Self {
            capacity_kwh: cfg.capacity_kwh,
            max_charge_kw: cfg.max_charge_kw,
            max_discharge_kw: cfg.max_discharge_kw,
            round_trip_efficiency: cfg.round_trip_efficiency,
            min_soc: cfg.min_soc,
        }
    }

    pub fn initial_state(cfg: &BatteryConfig) -> BatteryState {
        BatteryState {
            soc_pct: cfg.initial_soc,
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
        let actual = if clamped > 0.0 && state.soc_pct >= 1.0 {
            0.0
        } else if clamped < 0.0 && state.soc_pct <= self.min_soc {
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
        let new_soc = (state.soc_pct + energy_kwh / self.capacity_kwh).clamp(0.0, 1.0);
        (
            BatteryState {
                soc_pct: new_soc,
                actual_power_kw: actual,
            },
            actual,
        )
    }

    /// Point-in-time feasible power range.
    pub fn capability_inner(&self, state: &BatteryState) -> AssetCapability {
        AssetCapability {
            max_export_kw: if state.soc_pct <= self.min_soc {
                0.0
            } else {
                -self.max_discharge_kw
            },
            max_import_kw: if state.soc_pct >= 1.0 {
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
        m.insert("soc".into(), state.soc_pct);
        m.insert("capacity_kwh".into(), self.capacity_kwh);
        m.insert("max_charge_kw".into(), self.max_charge_kw);
        m.insert("max_discharge_kw".into(), self.max_discharge_kw);
        m.insert("min_soc".into(), self.min_soc);
        m
    }

    pub fn capabilities(&self, asset_id: &str, state: &BatteryState) -> AssetCapabilities {
        let cap = self.capability_inner(state);
        AssetCapabilities {
            asset_id: asset_id.to_string(),
            max_import_kw: cap.max_import_kw,
            max_export_kw: self.max_discharge_kw,
            is_flexible: true,
            energy_state: Some(EnergyState {
                current_kwh: state.soc_pct * self.capacity_kwh,
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

    pub fn reset(&self, state: &mut BatteryState, values: HashMap<String, f64>) {
        if let Some(&soc) = values.get("soc") {
            state.soc_pct = soc.clamp(0.0, 1.0);
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
        let mut soc = state.soc_pct;

        while t < end {
            let kw = if setpoint > 0.0 && soc >= 1.0 {
                0.0
            } else if setpoint < 0.0 && soc <= self.min_soc {
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
            t = t + Duration::seconds(60);
        }
        let end_kw = if setpoint > 0.0 && soc >= 1.0 {
            0.0
        } else if setpoint < 0.0 && soc <= self.min_soc {
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

    pub fn history(&self, timespan: Duration, history: &AssetHistoryBuffer) -> TimeSeries {
        super::history_from_buffer(timespan, history, Interpolation::Linear)
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
        let delta = (target - state.soc_pct).max(0.0);
        let kwh = delta * self.capacity_kwh;
        if kwh < 1e-6 {
            return None;
        }
        Some((kwh, desired_power_kw.unwrap_or(self.max_charge_kw)))
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
    use crate::profile::BatteryConfig;

    fn make_battery_cfg(initial_soc: f64) -> (Battery, BatteryState) {
        let cfg = BatteryConfig {
            id: "battery".to_string(),
            capacity_kwh: 10.0,
            max_charge_kw: 5.0,
            max_discharge_kw: 5.0,
            round_trip_efficiency: 0.95,
            min_soc: 0.1,
            initial_soc,
        };
        (Battery::from_config(&cfg), Battery::initial_state(&cfg))
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
        assert!((state.soc_pct - 1.0).abs() < 0.001);
        let (_, actual) = bat.step_inner(&state, 10.0, Duration::seconds(1));
        assert_eq!(actual, 0.0);
    }
}
