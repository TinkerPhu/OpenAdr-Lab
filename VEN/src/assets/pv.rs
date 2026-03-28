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
}
