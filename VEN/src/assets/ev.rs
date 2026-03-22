use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{
    Asset, AssetCapabilities, AssetCapability, AssetState, ControlDescriptor, ControlKind,
    EnergyState,
};
use crate::common::{Interpolation, TimeSeries};
use crate::controller::trace::AssetHistoryBuffer;
use crate::profile::EvConfig;

/// EV Charger config. Positive = charge (import), negative = V2G discharge (export).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvCharger {
    pub max_charge_kw: f64,
    pub max_discharge_kw: f64,
    pub battery_kwh: f64,
    pub soc_target: f64,
    pub default_charge_kw: f64,
    /// V2G floor; 0.0 if not specified in profile.
    pub min_soc: f64,
}

/// EV mutable state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvState {
    /// State of charge in [0.0, 1.0].
    pub soc_pct: f64,
    pub plugged: bool,
    /// Actual power last tick. Positive = charging (import). Negative = V2G (export).
    pub actual_power_kw: f64,
}

impl EvCharger {
    pub fn from_config(cfg: &EvConfig) -> Self {
        Self {
            max_charge_kw: cfg.max_charge_kw,
            max_discharge_kw: cfg.max_discharge_kw,
            battery_kwh: cfg.battery_kwh,
            soc_target: cfg.soc_target,
            default_charge_kw: cfg.default_charge_kw,
            min_soc: 0.0,
        }
    }

    pub fn initial_state(cfg: &EvConfig) -> EvState {
        EvState {
            soc_pct: cfg.initial_soc,
            plugged: true,
            actual_power_kw: cfg.max_charge_kw,
        }
    }

    /// Pure physics step. Returns (new_state, actual_power_kw).
    pub fn step_inner(&self, state: &EvState, setpoint_kw: f64, dt: Duration) -> (EvState, f64) {
        if !state.plugged {
            return (
                EvState {
                    actual_power_kw: 0.0,
                    ..state.clone()
                },
                0.0,
            );
        }
        let kw = setpoint_kw.clamp(-self.max_discharge_kw, self.max_charge_kw);
        let kw = if kw > 0.0 && state.soc_pct >= 1.0 {
            0.0
        } else if kw < 0.0 && state.soc_pct <= self.min_soc {
            0.0
        } else {
            kw
        };
        let dt_h = dt.num_milliseconds() as f64 / 3_600_000.0;
        let new_soc = (state.soc_pct + (kw * dt_h) / self.battery_kwh).clamp(0.0, 1.0);
        (
            EvState {
                soc_pct: new_soc,
                plugged: state.plugged,
                actual_power_kw: kw,
            },
            kw,
        )
    }

    /// Point-in-time feasible power range.
    pub fn capability_inner(&self, state: &EvState) -> AssetCapability {
        if !state.plugged {
            return AssetCapability {
                max_export_kw: 0.0,
                max_import_kw: 0.0,
            };
        }
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
        self.default_charge_kw
    }

    pub fn state_values(&self, state: &EvState) -> HashMap<String, f64> {
        let mut m = HashMap::new();
        m.insert("soc".into(), state.soc_pct);
        m.insert("plugged".into(), if state.plugged { 1.0 } else { 0.0 });
        m.insert("max_charge_kw".into(), self.max_charge_kw);
        m.insert("soc_target".into(), self.soc_target);
        m.insert("battery_kwh".into(), self.battery_kwh);
        m
    }

    pub fn capabilities(&self, asset_id: &str, state: &EvState) -> AssetCapabilities {
        AssetCapabilities {
            asset_id: asset_id.to_string(),
            max_import_kw: self.max_charge_kw,
            max_export_kw: self.max_discharge_kw,
            is_flexible: true,
            energy_state: Some(EnergyState {
                current_kwh: state.soc_pct * self.battery_kwh,
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

    pub fn reset(&self, state: &mut EvState, values: HashMap<String, f64>) {
        if let Some(&soc) = values.get("soc") {
            state.soc_pct = soc.clamp(0.0, 1.0);
        }
    }

    pub fn update_config(&mut self, values: HashMap<String, f64>) {
        if let Some(&v) = values.get("max_charge_kw") {
            self.max_charge_kw = v.max(0.0);
        }
    }

    pub fn forecast(&self, state: &EvState, timespan: Duration) -> TimeSeries {
        if timespan <= Duration::zero() {
            return TimeSeries::empty(Interpolation::Step);
        }
        let now = Utc::now();
        let power = if state.plugged {
            state.actual_power_kw
        } else {
            0.0
        };
        TimeSeries {
            samples: vec![(now, power), (now + timespan, power)],
            interpolation: Interpolation::Step,
        }
    }

    pub fn history(&self, timespan: Duration, history: &AssetHistoryBuffer) -> TimeSeries {
        super::history_from_buffer(timespan, history, Interpolation::Step)
    }

    pub fn default_comfort_rates(&self) -> Vec<crate::entities::asset::ComfortRate> {
        vec![
            crate::entities::asset::ComfortRate {
                fill: 0.0,
                max_marginal_price: 0.35,
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
        state: &EvState,
        target_soc: Option<f64>,
        desired_power_kw: Option<f64>,
    ) -> Option<(f64, f64)> {
        let target = target_soc.unwrap_or(self.soc_target);
        let delta = (target - state.soc_pct).max(0.0);
        let kwh = delta * self.battery_kwh;
        if kwh < 1e-6 {
            return None;
        }
        Some((kwh, desired_power_kw.unwrap_or(self.max_charge_kw)))
    }
}

impl Asset for EvCharger {
    fn step(&self, state: &AssetState, setpoint_kw: f64, dt: Duration) -> (AssetState, f64) {
        let AssetState::Ev(s) = state else {
            unreachable!("EvCharger/state mismatch")
        };
        let (ns, p) = self.step_inner(s, setpoint_kw, dt);
        (AssetState::Ev(ns), p)
    }

    fn capability(&self, state: &AssetState) -> AssetCapability {
        let AssetState::Ev(s) = state else {
            unreachable!()
        };
        self.capability_inner(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ev(plugged: bool, soc_pct: f64, actual_power_kw: f64) -> (EvCharger, EvState) {
        let cfg = EvCharger {
            max_charge_kw: 7.4,
            max_discharge_kw: 0.0,
            battery_kwh: 40.0,
            soc_target: 0.8,
            default_charge_kw: 7.4,
            min_soc: 0.0,
        };
        let state = EvState {
            soc_pct,
            plugged,
            actual_power_kw,
        };
        (cfg, state)
    }

    #[test]
    fn forecast_not_plugged_returns_zero() {
        let (ev, state) = make_ev(false, 0.5, 7.4);
        let series = ev.forecast(&state, Duration::seconds(3600));
        for (_, v) in &series.samples {
            assert_eq!(*v, 0.0, "Unplugged EV must return zero power");
        }
    }

    #[test]
    fn forecast_zero_timespan_returns_empty() {
        let (ev, state) = make_ev(true, 0.5, 7.4);
        let series = ev.forecast(&state, Duration::zero());
        assert!(series.samples.is_empty());
    }

    #[test]
    fn forecast_has_two_samples_with_boundary() {
        let (ev, state) = make_ev(true, 0.5, 7.4);
        let timespan = Duration::seconds(600);
        let before = Utc::now();
        let series = ev.forecast(&state, timespan);
        let after = Utc::now();
        assert_eq!(
            series.samples.len(),
            2,
            "Step forecast must have exactly 2 samples"
        );
        let last_ts = series.samples.last().unwrap().0;
        assert!(last_ts >= before + timespan && last_ts <= after + timespan);
    }

    #[test]
    fn ev_charges_and_stops_at_full() {
        let (ev, mut state) = make_ev(true, 0.99, 0.0);
        let ev = EvCharger {
            max_charge_kw: 10.0,
            battery_kwh: 10.0,
            ..ev
        };
        for _ in 0..1000 {
            let (ns, _) = ev.step_inner(&state, 10.0, Duration::seconds(1));
            state = ns;
        }
        assert!((state.soc_pct - 1.0).abs() < 0.001);
        let (_, actual) = ev.step_inner(&state, 10.0, Duration::seconds(1));
        assert_eq!(actual, 0.0);
    }

    #[test]
    fn ev_discharges_v2g_and_stops_at_empty() {
        let ev = EvCharger {
            max_charge_kw: 10.0,
            max_discharge_kw: 10.0,
            battery_kwh: 10.0,
            soc_target: 1.0,
            default_charge_kw: 0.0,
            min_soc: 0.0,
        };
        let mut state = EvState {
            soc_pct: 0.01,
            plugged: true,
            actual_power_kw: 0.0,
        };
        for _ in 0..1000 {
            let (ns, _) = ev.step_inner(&state, -10.0, Duration::seconds(1));
            state = ns;
        }
        assert!((state.soc_pct - 0.0).abs() < 0.001);
        let (_, actual) = ev.step_inner(&state, -10.0, Duration::seconds(1));
        assert_eq!(actual, 0.0);
    }
}
