use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{Asset, AssetCapability, AssetFlexibilityFloor, AssetState, ControlDescriptor};
use crate::common::{Interpolation, TimeSeries};
use crate::entities::asset_params::BatteryParams;

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
    pub soc: f64,
    /// Actual power last tick. Positive = charging (import). Negative = discharging (export).
    pub actual_power_kw: f64,
}

impl Battery {
    pub fn from_params(cfg: &BatteryParams) -> Self {
        Self {
            capacity_kwh: cfg.capacity_kwh,
            max_charge_kw: cfg.max_charge_kw,
            max_discharge_kw: cfg.max_discharge_kw,
            round_trip_efficiency: cfg.round_trip_efficiency,
            min_soc: cfg.min_soc,
        }
    }

    pub fn initial_state(cfg: &BatteryParams) -> BatteryState {
        BatteryState {
            soc: cfg.initial_soc,
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
        let actual = if (clamped > 0.0 && state.soc >= 1.0)
            || (clamped < 0.0 && state.soc <= self.min_soc)
        {
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
        let new_soc = (state.soc + energy_kwh / self.capacity_kwh).clamp(0.0, 1.0);
        (
            BatteryState {
                soc: new_soc,
                actual_power_kw: actual,
            },
            actual,
        )
    }

    /// Point-in-time feasible power range.
    pub fn capability_inner(&self, state: &BatteryState) -> AssetCapability {
        AssetCapability {
            max_export_kw: if state.soc <= self.min_soc {
                0.0
            } else {
                -self.max_discharge_kw
            },
            max_import_kw: if state.soc >= 1.0 {
                0.0
            } else {
                self.max_charge_kw
            },
        }
    }

    /// Continuously controllable in both directions — can always idle at 0,
    /// no minimum-nonzero commitment exists for battery.
    pub fn flexibility_floor_inner(&self, _state: &BatteryState) -> AssetFlexibilityFloor {
        AssetFlexibilityFloor {
            min_export_kw: 0.0,
            min_import_kw: 0.0,
        }
    }

    pub fn default_setpoint(&self) -> f64 {
        0.0 // hold by default; dispatcher controls
    }

    pub fn state_values(&self, state: &BatteryState) -> HashMap<String, f64> {
        let mut m = HashMap::new();
        m.insert("soc".into(), state.soc);
        m.insert("capacity_kwh".into(), self.capacity_kwh);
        m.insert("max_charge_kw".into(), self.max_charge_kw);
        m.insert("max_discharge_kw".into(), self.max_discharge_kw);
        m.insert("min_soc".into(), self.min_soc);
        m
    }

    /// State values for a future MILP time slot, given the battery energy stored
    /// at the start of that slot (kWh). Returns `{"soc": <0..1>}`.
    pub fn future_state_values(&self, e_kwh: f64) -> HashMap<String, f64> {
        let soc = (e_kwh / self.capacity_kwh).clamp(0.0, 1.0);
        HashMap::from([("soc".into(), soc)])
    }

    pub fn control_schema(&self) -> Vec<ControlDescriptor> {
        vec![]
    }

    pub fn reset(&self, state: &mut BatteryState, values: HashMap<String, f64>) {
        if let Some(&soc) = values.get("soc") {
            state.soc = soc.clamp(0.0, 1.0);
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
        let mut soc = state.soc;

        while t < end {
            let kw = if (setpoint > 0.0 && soc >= 1.0) || (setpoint < 0.0 && soc <= self.min_soc) {
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
            t += Duration::seconds(60);
        }
        let end_kw = if (setpoint > 0.0 && soc >= 1.0) || (setpoint < 0.0 && soc <= self.min_soc) {
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
        let delta = (target - state.soc).max(0.0);
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

    fn flexibility_floor(&self, state: &AssetState) -> AssetFlexibilityFloor {
        let AssetState::Battery(s) = state else {
            unreachable!()
        };
        self.flexibility_floor_inner(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_battery_cfg(initial_soc: f64) -> (Battery, BatteryState) {
        let cfg = BatteryParams {
            id: "battery".to_string(),
            capacity_kwh: 10.0,
            max_charge_kw: 5.0,
            max_discharge_kw: 5.0,
            round_trip_efficiency: 0.95,
            min_soc: 0.1,
            initial_soc,
            c_terminal_eur_kwh: None,
        };
        (Battery::from_params(&cfg), Battery::initial_state(&cfg))
    }

    #[test]
    fn flexibility_floor_is_always_zero_regardless_of_soc() {
        for soc in [0.05, 0.5, 1.0] {
            // 0.05 is below min_soc=0.1; 1.0 is fully charged.
            let (bat, state) = make_battery_cfg(soc);
            let floor = bat.flexibility_floor_inner(&state);
            assert_eq!(floor.min_export_kw, 0.0, "soc={soc}");
            assert_eq!(floor.min_import_kw, 0.0, "soc={soc}");
        }
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
        assert!((state.soc - 1.0).abs() < 0.001);
        let (_, actual) = bat.step_inner(&state, 10.0, Duration::seconds(1));
        assert_eq!(actual, 0.0);
    }

    // T007: Battery::future_state_values returns correct soc.
    #[test]
    fn future_state_values_mid_charge() {
        let bat = Battery {
            capacity_kwh: 10.0,
            max_charge_kw: 5.0,
            max_discharge_kw: 5.0,
            round_trip_efficiency: 0.9,
            min_soc: 0.1,
        };
        let vals = bat.future_state_values(5.0); // 5 kWh of 10 kWh capacity → SoC = 0.5
        let soc = vals["soc"];
        assert!((soc - 0.5).abs() < 1e-9, "expected soc=0.5, got {soc}");
    }

    #[test]
    fn future_state_values_clamps_to_zero() {
        let bat = Battery {
            capacity_kwh: 10.0,
            max_charge_kw: 5.0,
            max_discharge_kw: 5.0,
            round_trip_efficiency: 0.9,
            min_soc: 0.1,
        };
        let vals = bat.future_state_values(-1.0);
        assert_eq!(vals["soc"], 0.0);
    }

    #[test]
    fn future_state_values_clamps_to_one() {
        let bat = Battery {
            capacity_kwh: 10.0,
            max_charge_kw: 5.0,
            max_discharge_kw: 5.0,
            round_trip_efficiency: 0.9,
            min_soc: 0.1,
        };
        let vals = bat.future_state_values(15.0);
        assert_eq!(vals["soc"], 1.0);
    }
}

#[cfg(test)]
mod param_tests {
    use super::*;

    #[test]
    fn battery_params_default_soc() {
        assert!((BatteryParams::default().initial_soc - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn battery_params_custom_capacity() {
        let params = BatteryParams {
            capacity_kwh: 20.0,
            ..BatteryParams::default()
        };
        assert!((params.capacity_kwh - 20.0).abs() < f64::EPSILON);
    }
}
