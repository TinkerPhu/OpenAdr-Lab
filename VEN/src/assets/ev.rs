use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{Asset, AssetCapability, AssetState, ControlDescriptor, ControlKind};
use crate::common::{Interpolation, TimeSeries};
use crate::entities::asset_params::EvParams;

/// EV Charger config. Positive = charge (import), negative = V2G discharge (export).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvCharger {
    pub max_charge_kw: f64,
    pub max_discharge_kw: f64,
    pub battery_kwh: f64,
    /// Active SOC ceiling — charging stops at this level (BMS limit). Overridable at runtime.
    pub soc_target: f64,
    /// Original profile value — used for snap-back when inject override is released.
    pub soc_target_profile: f64,
    pub default_charge_kw: f64,
    /// V2G floor; 0.0 if not specified in profile.
    pub min_soc: f64,
}

/// EV mutable state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvState {
    /// State of charge in [0.0, 1.0].
    pub soc: f64,
    pub plugged: bool,
    /// Actual power last tick. Positive = charging (import). Negative = V2G (export).
    pub actual_power_kw: f64,
}

impl EvCharger {
    pub fn from_params(cfg: &EvParams) -> Self {
        Self {
            max_charge_kw: cfg.max_charge_kw,
            max_discharge_kw: cfg.max_discharge_kw,
            battery_kwh: cfg.battery_kwh,
            soc_target: cfg.soc_target,
            soc_target_profile: cfg.soc_target,
            default_charge_kw: cfg.default_charge_kw,
            min_soc: 0.0,
        }
    }

    pub fn initial_state(cfg: &EvParams) -> EvState {
        EvState {
            soc: cfg.initial_soc,
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
        let kw = if (kw > 0.0 && state.soc >= self.soc_target)
            || (kw < 0.0 && state.soc <= self.min_soc)
        {
            0.0
        } else {
            kw
        };
        let dt_h = dt.num_milliseconds() as f64 / 3_600_000.0;
        let new_soc = (state.soc + (kw * dt_h) / self.battery_kwh).clamp(0.0, 1.0);
        (
            EvState {
                soc: new_soc,
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
            max_export_kw: if state.soc <= self.min_soc {
                0.0
            } else {
                -self.max_discharge_kw
            },
            max_import_kw: if state.soc >= self.soc_target {
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
        m.insert("soc".into(), state.soc);
        m.insert("plugged".into(), if state.plugged { 1.0 } else { 0.0 });
        m.insert("max_charge_kw".into(), self.max_charge_kw);
        m.insert("soc_target".into(), self.soc_target);
        m.insert("battery_kwh".into(), self.battery_kwh);
        m
    }

    /// Compute EV SoC trajectory from MILP charge-power schedule.
    ///
    /// Returns a `Vec<f64>` of length `n + 1` where index `t` is the SoC at the
    /// **start** of slot `t` (index `n` is the SoC at the end of the last slot).
    /// `p_ev_kw[t]` is net charge power (kW) during slot `t`, `dt_h` is slot
    /// duration in hours.  Values are clamped to `[0.0, 1.0]`.
    pub fn soc_trajectory(p_ev_kw: &[f64], soc_init: f64, battery_kwh: f64, dt_h: f64) -> Vec<f64> {
        let n = p_ev_kw.len();
        let mut traj = Vec::with_capacity(n + 1);
        traj.push(soc_init.clamp(0.0, 1.0));
        for t in 0..n {
            let next = traj[t] + p_ev_kw[t] * dt_h / battery_kwh;
            traj.push(next.clamp(0.0, 1.0));
        }
        traj
    }

    /// State values for a future MILP time slot given the SoC at the start of
    /// that slot. Returns `{"soc": <0..1>}`.
    pub fn future_state_values_at(soc: f64) -> HashMap<String, f64> {
        HashMap::from([("soc".into(), soc.clamp(0.0, 1.0))])
    }

    pub fn control_schema(&self) -> Vec<ControlDescriptor> {
        vec![
            ControlDescriptor {
                key: "ev_plugged".into(),
                label: "Plugged In".into(),
                kind: ControlKind::Switch,
                min: None,
                max: None,
                unit: "".into(),
                display_scale: None,
            },
            ControlDescriptor {
                key: "ev_soc_target".into(),
                label: "Charge Target".into(),
                kind: ControlKind::Slider,
                min: Some(0.1),
                max: Some(1.0),
                unit: "%".into(),
                display_scale: Some(100.0),
            },
        ]
    }

    pub fn reset(&self, state: &mut EvState, values: HashMap<String, f64>) {
        if let Some(&soc) = values.get("soc") {
            state.soc = soc.clamp(0.0, 1.0);
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
        let delta = (target - state.soc).max(0.0);
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

    fn make_ev(plugged: bool, soc: f64, actual_power_kw: f64) -> (EvCharger, EvState) {
        let cfg = EvCharger {
            max_charge_kw: 7.4,
            max_discharge_kw: 0.0,
            battery_kwh: 40.0,
            soc_target: 0.8,
            soc_target_profile: 0.8,
            default_charge_kw: 7.4,
            min_soc: 0.0,
        };
        let state = EvState {
            soc,
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
            soc_target: 1.0,
            soc_target_profile: 1.0,
            battery_kwh: 10.0,
            ..ev
        };
        for _ in 0..1000 {
            let (ns, _) = ev.step_inner(&state, 10.0, Duration::seconds(1));
            state = ns;
        }
        assert!((state.soc - 1.0).abs() < 0.001);
        let (_, actual) = ev.step_inner(&state, 10.0, Duration::seconds(1));
        assert_eq!(actual, 0.0);
    }

    #[test]
    fn ev_stops_charging_at_soc_target() {
        // soc_target = 0.8 — charging must stop there, not at 1.0
        let (ev, mut state) = make_ev(true, 0.0, 0.0);
        for _ in 0..100_000 {
            let (ns, _) = ev.step_inner(&state, 7.4, Duration::seconds(1));
            state = ns;
        }
        assert!(
            state.soc <= 0.8 + 0.01,
            "soc should not exceed soc_target (0.8), got {}",
            state.soc
        );
        let (_, actual) = ev.step_inner(&state, 7.4, Duration::seconds(1));
        assert_eq!(actual, 0.0, "charging must stop at soc_target");
    }

    #[test]
    fn ev_discharges_v2g_and_stops_at_empty() {
        let ev = EvCharger {
            max_charge_kw: 10.0,
            max_discharge_kw: 10.0,
            battery_kwh: 10.0,
            soc_target: 1.0,
            soc_target_profile: 1.0,
            default_charge_kw: 0.0,
            min_soc: 0.0,
        };
        let mut state = EvState {
            soc: 0.01,
            plugged: true,
            actual_power_kw: 0.0,
        };
        for _ in 0..1000 {
            let (ns, _) = ev.step_inner(&state, -10.0, Duration::seconds(1));
            state = ns;
        }
        assert!((state.soc - 0.0).abs() < 0.001);
        let (_, actual) = ev.step_inner(&state, -10.0, Duration::seconds(1));
        assert_eq!(actual, 0.0);
    }

    #[test]
    fn ev_capability_zero_at_soc_target() {
        // When soc == soc_target the BMS ceiling is hit: capability must report
        // max_import_kw = 0 so the planner fires SocCeiling, not a phantom allocation.
        let (ev, state) = make_ev(true, 0.8, 0.0); // soc_target = 0.8 in make_ev
        let cap = ev.capability_inner(&state);
        assert_eq!(
            cap.max_import_kw, 0.0,
            "capability must be 0 when soc equals soc_target"
        );
    }

    #[test]
    fn ev_capability_positive_below_soc_target() {
        let (ev, state) = make_ev(true, 0.5, 0.0); // soc 0.5 < soc_target 0.8
        let cap = ev.capability_inner(&state);
        assert!(
            cap.max_import_kw > 0.0,
            "capability must be positive when soc < soc_target"
        );
        assert!((cap.max_import_kw - ev.max_charge_kw).abs() < 1e-9);
    }

    // T012: EvCharger::soc_trajectory and future_state_values_at.
    #[test]
    fn soc_trajectory_charges_monotonically() {
        // 5 slots of 1 kW charging, 10 kWh battery, dt_h = 1h → each slot +0.1 SoC
        let p_ev = vec![1.0_f64; 5];
        let traj = EvCharger::soc_trajectory(&p_ev, 0.0, 10.0, 1.0);
        assert_eq!(traj.len(), 6);
        for i in 1..=5 {
            assert!(traj[i] > traj[i - 1], "SoC must increase during charging");
        }
        assert!(
            (traj[5] - 0.5).abs() < 1e-9,
            "expected final soc=0.5, got {}",
            traj[5]
        );
    }

    #[test]
    fn soc_trajectory_clamps_at_one() {
        // Over-charge scenario: 1000 slots of 10 kW charging
        let p_ev = vec![10.0_f64; 1000];
        let traj = EvCharger::soc_trajectory(&p_ev, 0.5, 10.0, 1.0);
        assert_eq!(*traj.last().unwrap(), 1.0);
    }

    #[test]
    fn future_state_values_at_returns_soc() {
        let vals = EvCharger::future_state_values_at(0.65);
        let soc = vals["soc"];
        assert!((soc - 0.65).abs() < 1e-9, "expected soc=0.65, got {soc}");
    }

    #[test]
    fn future_state_values_at_clamps() {
        assert_eq!(EvCharger::future_state_values_at(-0.1)["soc"], 0.0);
        assert_eq!(EvCharger::future_state_values_at(1.5)["soc"], 1.0);
    }
}

#[cfg(test)]
mod param_tests {
    use super::*;

    #[test]
    fn ev_params_default_values() {
        assert!((EvParams::default().max_charge_kw - 7.4).abs() < f64::EPSILON);
    }

    #[test]
    fn ev_params_min_charge_enforced() {
        let params = EvParams {
            min_charge_kw: 2.0,
            ..EvParams::default()
        };
        assert!((params.min_charge_kw - 2.0).abs() < f64::EPSILON);
    }
}
