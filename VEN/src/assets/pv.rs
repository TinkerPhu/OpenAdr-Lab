use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::common::{Interpolation, TimeSeries};
use crate::controller::trace::AssetHistoryBuffer;
use crate::profile::PvConfig;
use super::{AssetCapabilities, ControlDescriptor, ControlKind, TickEnvironment};

/// PV Inverter: generates power.
/// `current_kw` stores the positive generation value (internal).
/// Sign convention (export = negative) is applied via `power_kw` in the API response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PvInverter {
    pub rated_kw: f64,
    pub irradiance: f64,              // 0.0..1.0
    pub export_limit_kw: Option<f64>, // active cap; None = no limit
    pub current_kw: f64,              // generation magnitude (positive)
}

impl PvInverter {
    pub fn from_config(cfg: &PvConfig) -> Self {
        Self {
            rated_kw: cfg.rated_kw,
            irradiance: 0.0,
            export_limit_kw: None,
            current_kw: 0.0,
        }
    }

    /// `setpoint` encodes export limit: f64::MAX (or very large) = no limit, else kW cap.
    /// Reads "hour_of_day" and "pv_irradiance" from env.
    pub fn update(&mut self, _dt_s: f64, setpoint: f64, env: &TickEnvironment) -> f64 {
        let hour = env.get("hour_of_day").copied().unwrap_or(12.0);
        let irradiance_override = env.get("pv_irradiance").copied();

        self.irradiance = irradiance_override.unwrap_or_else(|| {
            if hour >= 6.0 && hour <= 18.0 {
                let angle = std::f64::consts::PI * (hour - 6.0) / 12.0;
                angle.sin()
            } else {
                0.0
            }
        });

        let natural_output = self.rated_kw * self.irradiance;
        let limit_kw = if setpoint >= f64::MAX * 0.5 {
            None
        } else {
            Some(setpoint.max(0.0))
        };
        let output = match limit_kw {
            Some(limit) => natural_output.min(limit),
            None => natural_output,
        };
        self.export_limit_kw = limit_kw;
        self.current_kw = output;
        output
    }

    pub fn forecast(&self, timespan: Duration) -> TimeSeries {
        if timespan <= Duration::zero() {
            return TimeSeries::empty(Interpolation::Linear);
        }
        let now = Utc::now();
        let end = now + timespan;
        let total_s = timespan.num_seconds();
        let mut samples: Vec<(chrono::DateTime<Utc>, f64)> = Vec::new();

        // One sample per minute across the timespan.
        let mut t = now;
        while t < end {
            samples.push((t, self.irradiance_at(t)));
            t = t + Duration::seconds(60);
        }
        // Mandatory boundary point at now + timespan.
        samples.push((end, self.irradiance_at(end)));

        // Deduplicate if the last regular sample landed on the boundary.
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

    /// Compute power output at a given timestamp using the sinusoidal irradiation model.
    /// Returns a negative value (export convention).
    fn irradiance_at(&self, ts: chrono::DateTime<Utc>) -> f64 {
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
            Some(limit) => natural_kw.min(limit),
            None => natural_kw,
        };
        -limited_kw // negative = export
    }

    pub fn history(&self, timespan: Duration, history: &AssetHistoryBuffer) -> TimeSeries {
        super::history_from_buffer(timespan, history, Interpolation::Linear)
    }

    pub fn state_values(&self) -> HashMap<String, f64> {
        let mut m = HashMap::new();
        m.insert("irradiance".into(), self.irradiance);
        m.insert("rated_kw".into(), self.rated_kw);
        if let Some(lim) = self.export_limit_kw {
            m.insert("export_limit_kw".into(), lim);
        }
        m
    }

    pub fn default_setpoint(&self) -> f64 {
        f64::MAX // no export limit by default
    }

    pub fn capabilities(&self, asset_id: &str) -> AssetCapabilities {
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
            },
            ControlDescriptor {
                key: "pv_force_export_limit_kw".into(),
                label: "Export Limit".into(),
                kind: ControlKind::NumberInput,
                min: Some(0.0),
                max: Some(self.rated_kw * 2.0),
                unit: "kW".into(),
            },
        ]
    }

    pub fn reset(&mut self, _values: HashMap<String, f64>) {}

    pub fn update_config(&mut self, values: HashMap<String, f64>) {
        if let Some(&v) = values.get("rated_kw") {
            self.rated_kw = v.max(0.0);
        }
    }

    pub fn default_comfort_rates(&self) -> Vec<crate::entities::asset::ComfortRate> {
        vec![
            crate::entities::asset::ComfortRate { fill: 0.0, max_marginal_price: 0.0, max_marginal_co2: 0.0 },
            crate::entities::asset::ComfortRate { fill: 1.0, max_marginal_price: 0.0, max_marginal_co2: 0.0 },
        ]
    }

    pub fn default_completion_policy(&self) -> crate::entities::asset::CompletionPolicy {
        crate::entities::asset::CompletionPolicy::Stop
    }

    pub fn default_post_deadline_comfort_bid(&self) -> Option<f64> {
        None
    }

    pub fn generation_w(&self) -> f64 {
        self.current_kw * 1000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_hour(h: f64) -> TickEnvironment {
        let mut e = HashMap::new();
        e.insert("hour_of_day".into(), h);
        e
    }

    fn env_irradiance(v: f64) -> TickEnvironment {
        let mut e = HashMap::new();
        e.insert("pv_irradiance".into(), v);
        e
    }

    fn make_pv(rated_kw: f64) -> PvInverter {
        PvInverter { rated_kw, irradiance: 0.0, export_limit_kw: None, current_kw: 0.0 }
    }

    #[test]
    fn pv_sinusoidal_model() {
        let cfg = PvConfig { id: "pv".to_string(), rated_kw: 10.0 };
        let mut pv = PvInverter::from_config(&cfg);
        let kw = pv.update(1.0, f64::MAX, &env_hour(12.0));
        assert!((kw - 10.0).abs() < 0.01, "noon should be ~10kW, got {kw}");
        let kw = pv.update(1.0, f64::MAX, &env_hour(0.0));
        assert_eq!(kw, 0.0);
        let kw = pv.update(1.0, 0.0, &env_hour(12.0));
        assert_eq!(kw, 0.0);
        let kw = pv.update(1.0, 5.0, &env_hour(12.0));
        assert!((kw - 5.0).abs() < 0.01);
        let kw = pv.update(1.0, f64::MAX, &env_irradiance(1.0));
        assert!((kw - 10.0).abs() < 0.01);
    }

    #[test]
    fn forecast_zero_timespan_returns_empty() {
        let pv = make_pv(5.0);
        let series = pv.forecast(Duration::zero());
        assert!(series.samples.is_empty(), "Zero timespan must return empty series");
    }

    #[test]
    fn forecast_has_boundary_point_at_end() {
        let pv = make_pv(5.0);
        let timespan = Duration::seconds(300); // 5 minutes
        let before = chrono::Utc::now();
        let series = pv.forecast(timespan);
        let after = chrono::Utc::now();
        assert!(!series.samples.is_empty(), "Non-zero timespan must produce samples");
        let last_ts = series.samples.last().unwrap().0;
        let expected_min = before + timespan;
        let expected_max = after + timespan;
        assert!(
            last_ts >= expected_min && last_ts <= expected_max,
            "Boundary point {last_ts} must be at now+timespan (range [{expected_min}, {expected_max}])"
        );
    }

    #[test]
    fn forecast_samples_ascending() {
        let pv = make_pv(5.0);
        let series = pv.forecast(Duration::seconds(120));
        let timestamps: Vec<_> = series.samples.iter().map(|(t, _)| t).collect();
        for i in 1..timestamps.len() {
            assert!(timestamps[i] > timestamps[i - 1], "Timestamps must be strictly ascending");
        }
    }

    #[test]
    fn forecast_all_negative_at_noon_for_rated_pv() {
        // At noon, PV exports → all values should be ≤ 0
        let pv = make_pv(5.0);
        let series = pv.forecast(Duration::seconds(60));
        for (ts, v) in &series.samples {
            assert!(*v <= 0.0, "PV value at {ts} should be ≤ 0 (export), got {v}");
        }
    }

    #[test]
    fn forecast_rated_zero_returns_all_zero() {
        let pv = make_pv(0.0); // no PV
        let series = pv.forecast(Duration::seconds(300));
        for (_, v) in &series.samples {
            assert_eq!(*v, 0.0, "Zero-rated PV must produce all-zero series");
        }
    }
}
