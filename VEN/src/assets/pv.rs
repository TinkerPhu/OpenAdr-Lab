use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{
    Asset, AssetCapabilities, AssetCapability, AssetState, ControlDescriptor, ControlKind,
};
use crate::common::{Interpolation, TimeSeries};
use crate::profile::PvConfig;

/// PV Inverter config. Generates power (export = negative).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PvInverter {
    pub rated_kw: f64,
    /// Active export limit in kW (≤ 0); None = no curtailment limit.
    pub export_limit_kw: Option<f64>,
    /// [0.0, 1.0]; set each tick by sim from SimInjectState or time-based model. NOT from YAML.
    pub irradiance: f64,
}

/// PV mutable state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PvState {
    /// Actual power last tick. Always ≤ 0 (PV only exports). Unit: kW.
    pub actual_power_kw: f64,
}

impl PvInverter {
    pub fn from_config(cfg: &PvConfig) -> Self {
        Self {
            rated_kw: cfg.rated_kw,
            export_limit_kw: None,
            irradiance: 0.0,
        }
    }

    pub fn initial_state(_cfg: &PvConfig) -> PvState {
        PvState {
            actual_power_kw: 0.0,
        }
    }

    /// Pure physics step. Ignores setpoint (non-curtailable in Phase A).
    /// Reads `self.irradiance` (set by sim loop each tick before calling).
    pub fn step_inner(&self, _state: &PvState, _setpoint_kw: f64, _dt: Duration) -> (PvState, f64) {
        let raw_kw = -(self.rated_kw * self.irradiance); // negative = export
        let actual_kw = self
            .export_limit_kw
            .map(|lim| raw_kw.max(lim)) // lim ≤ 0; max() clamps to less export
            .unwrap_or(raw_kw);
        (
            PvState {
                actual_power_kw: actual_kw,
            },
            actual_kw,
        )
    }

    /// Non-curtailable point-range capability.
    pub fn capability_inner(&self, state: &PvState) -> AssetCapability {
        AssetCapability {
            max_export_kw: state.actual_power_kw, // e.g. -2.0
            max_import_kw: state.actual_power_kw, // same — is_fixed() = true
        }
    }

    pub fn default_setpoint(&self) -> f64 {
        f64::MAX // no export limit by default
    }

    pub fn state_values(&self, _state: &PvState) -> HashMap<String, f64> {
        let mut m = HashMap::new();
        m.insert("irradiance".into(), self.irradiance);
        m.insert("rated_kw".into(), self.rated_kw);
        if let Some(lim) = self.export_limit_kw {
            m.insert("export_limit_kw".into(), lim);
        }
        m
    }

    pub fn capabilities(&self, asset_id: &str, _state: &PvState) -> AssetCapabilities {
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
                display_scale: None,
            },
            ControlDescriptor {
                key: "pv_irradiance_alpha".into(),
                label: "Blend-back Speed".into(),
                kind: ControlKind::Slider,
                min: Some(0.01),
                max: Some(1.0),
                unit: "".into(),
                display_scale: None,
            },
        ]
    }

    pub fn reset(&self, _state: &mut PvState, _values: HashMap<String, f64>) {}

    pub fn update_config(&mut self, values: HashMap<String, f64>) {
        if let Some(&v) = values.get("rated_kw") {
            self.rated_kw = v.max(0.0);
        }
    }

    pub fn forecast(&self, _state: &PvState, timespan: Duration) -> TimeSeries {
        if timespan <= Duration::zero() {
            return TimeSeries::empty(Interpolation::Linear);
        }
        let now = Utc::now();
        let end = now + timespan;
        let mut samples: Vec<(DateTime<Utc>, f64)> = Vec::new();

        let mut t = now;
        while t < end {
            samples.push((t, self.irradiance_at(t)));
            t = t + Duration::seconds(60);
        }
        samples.push((end, self.irradiance_at(end)));

        if samples.len() >= 2 {
            let n = samples.len();
            if (samples[n - 2].0 - samples[n - 1].0).num_seconds().abs() < 1 {
                samples.truncate(n - 1);
                samples.push((end, self.irradiance_at(end)));
            }
        }

        TimeSeries {
            samples,
            interpolation: Interpolation::Linear,
        }
    }

    fn irradiance_at(&self, ts: DateTime<Utc>) -> f64 {
        use chrono::Timelike;
        let hour = ts.hour() as f64 + ts.minute() as f64 / 60.0;
        let irradiance = if hour >= 6.0 && hour <= 18.0 {
            let angle = std::f64::consts::PI * (hour - 6.0) / 12.0;
            angle.sin()
        } else {
            0.0
        };
        let natural_kw = self.rated_kw * irradiance;
        let limited_kw = match self.export_limit_kw {
            Some(limit) => natural_kw.min(limit.abs()), // limit stored as negative; abs for min()
            None => natural_kw,
        };
        -limited_kw // negative = export
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

impl Asset for PvInverter {
    fn step(&self, state: &AssetState, setpoint_kw: f64, dt: Duration) -> (AssetState, f64) {
        let AssetState::Pv(s) = state else {
            unreachable!("PvInverter/state mismatch")
        };
        let (ns, p) = self.step_inner(s, setpoint_kw, dt);
        (AssetState::Pv(ns), p)
    }

    fn capability(&self, state: &AssetState) -> AssetCapability {
        let AssetState::Pv(s) = state else {
            unreachable!()
        };
        self.capability_inner(s)
    }

    /// Override: use time-varying irradiance_at(t) for each future step so the
    /// planner sees a sin-curve forecast rather than the frozen current irradiance.
    fn capability_trajectory(
        &self,
        _initial: &AssetState,
        duration: Duration,
        resolution: Duration,
    ) -> Vec<(DateTime<Utc>, AssetCapability)> {
        let now = Utc::now();
        let n = (duration.num_seconds() / resolution.num_seconds().max(1)) as usize;
        let mut result = Vec::with_capacity(n);
        for i in 1..=n {
            let t = now + resolution * i as i32;
            // irradiance_at uses the sin model (ignores self.irradiance override)
            let power_kw = self.irradiance_at(t); // negative = export
            result.push((t, AssetCapability {
                max_export_kw: power_kw,
                max_import_kw: power_kw, // non-curtailable: same bound in both directions
            }));
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pv(rated_kw: f64) -> (PvInverter, PvState) {
        (
            PvInverter {
                rated_kw,
                irradiance: 0.0,
                export_limit_kw: None,
            },
            PvState {
                actual_power_kw: 0.0,
            },
        )
    }

    #[test]
    fn forecast_zero_timespan_returns_empty() {
        let (pv, state) = make_pv(5.0);
        let series = pv.forecast(&state, Duration::zero());
        assert!(
            series.samples.is_empty(),
            "Zero timespan must return empty series"
        );
    }

    #[test]
    fn forecast_has_boundary_point_at_end() {
        let (pv, state) = make_pv(5.0);
        let timespan = Duration::seconds(300);
        let before = Utc::now();
        let series = pv.forecast(&state, timespan);
        let after = Utc::now();
        assert!(
            !series.samples.is_empty(),
            "Non-zero timespan must produce samples"
        );
        let last_ts = series.samples.last().unwrap().0;
        assert!(
            last_ts >= before + timespan && last_ts <= after + timespan,
            "Boundary point must be at now+timespan"
        );
    }

    #[test]
    fn forecast_samples_ascending() {
        let (pv, state) = make_pv(5.0);
        let series = pv.forecast(&state, Duration::seconds(120));
        let timestamps: Vec<_> = series.samples.iter().map(|(t, _)| t).collect();
        for i in 1..timestamps.len() {
            assert!(
                timestamps[i] > timestamps[i - 1],
                "Timestamps must be strictly ascending"
            );
        }
    }

    #[test]
    fn forecast_rated_zero_returns_all_zero() {
        let (pv, state) = make_pv(0.0);
        let series = pv.forecast(&state, Duration::seconds(300));
        for (_, v) in &series.samples {
            assert_eq!(*v, 0.0, "Zero-rated PV must produce all-zero series");
        }
    }

    #[test]
    fn step_generates_at_noon_irradiance() {
        let (mut pv, state) = make_pv(10.0);
        pv.irradiance = 1.0; // noon
        let (new_state, power) = pv.step_inner(&state, 0.0, Duration::seconds(1));
        assert!(
            (power + 10.0).abs() < 0.01,
            "Should export ~10 kW at full irradiance"
        );
        assert!((new_state.actual_power_kw + 10.0).abs() < 0.01);
    }

    // ── capability_trajectory tests ──────────────────────────────────────────

    #[test]
    fn capability_trajectory_uses_sin_model_not_flat_irradiance() {
        // self.irradiance is frozen at 0.5 (would give −5 kW flat if used).
        // The override returns time-varying values from irradiance_at(t).
        let mut pv = PvInverter {
            rated_kw: 10.0,
            irradiance: 0.5,
            export_limit_kw: None,
        };
        // Set an obviously wrong irradiance to confirm it is NOT used.
        pv.irradiance = 0.5;
        let state = AssetState::Pv(PvState { actual_power_kw: -5.0 });

        // 24-hour trajectory at 1-hour resolution — always spans a full day cycle.
        let traj = pv.capability_trajectory(&state, Duration::hours(24), Duration::hours(1));
        assert_eq!(traj.len(), 24);

        // Each point must match irradiance_at(t), not −rated_kw * self.irradiance.
        let flat_wrong = -pv.rated_kw * pv.irradiance; // −5.0
        for (t, cap) in &traj {
            let expected = pv.irradiance_at(*t);
            assert!(
                (cap.max_export_kw - expected).abs() < 1e-9,
                "at {t}: expected {expected:.3} (sin model), got {:.3}", cap.max_export_kw
            );
            // Values must not all be −5.0 (proving sin model is used, not flat override)
            // (We check this in aggregate below.)
            let _ = flat_wrong;
        }

        // At least some daytime points are non-zero and some night points are zero.
        // 24 h from now always contains at least 6 daytime hours (6am-6pm UTC window).
        let non_zero = traj.iter().filter(|(_, c)| c.max_export_kw.abs() > 1e-6).count();
        assert!(non_zero > 0, "24-h trajectory must include some daytime generation");

        // All PV values must be ≤ 0 (export only).
        for (_, cap) in &traj {
            assert!(cap.max_export_kw <= 1e-9, "PV trajectory must be non-positive");
        }

        // Not all identical to flat_wrong — proves sin model is used.
        let all_flat = traj.iter().all(|(_, c)| (c.max_export_kw - flat_wrong).abs() < 1e-6);
        assert!(!all_flat, "trajectory must NOT be flat at self.irradiance × rated_kw");
    }

    #[test]
    fn capability_trajectory_respects_rated_kw() {
        let pv = PvInverter {
            rated_kw: 8.0,
            irradiance: 0.0,
            export_limit_kw: None,
        };
        let state = AssetState::Pv(PvState { actual_power_kw: 0.0 });
        let traj = pv.capability_trajectory(&state, Duration::hours(24), Duration::hours(1));
        for (_, cap) in &traj {
            assert!(
                cap.max_export_kw >= -8.0 - 1e-9,
                "export must not exceed rated_kw=8.0, got {}", cap.max_export_kw
            );
        }
    }

    #[test]
    fn capability_trajectory_ascending_timestamps() {
        let (pv, state_inner) = make_pv(5.0);
        let state = AssetState::Pv(state_inner);
        let traj = pv.capability_trajectory(&state, Duration::hours(6), Duration::hours(1));
        assert_eq!(traj.len(), 6);
        for i in 1..traj.len() {
            assert!(
                traj[i].0 > traj[i - 1].0,
                "trajectory timestamps must be strictly ascending"
            );
        }
    }
}
