//! How much a weather-sourced forecast hour is trusted: the one definition of a forecast's `confidence`.
//!
//! Owns: the per-hour figure `base_confidence(age_h) x (1 - irradiance_variability)` and its mean over a
//! series. The base is a starting default (linear decay to a 0.2 floor at the 48 h horizon), not a measured
//! curve; tune it once real accuracy data exists. Reads: values handed in. Must never: read a clock.

use crate::weather::WeatherForecastSample;

/// `base_confidence(age_h) x (1 - irradiance_variability)` for one sample. A missing variability counts
/// as maximal uncertainty, never as a stable sky.
pub fn slot_confidence(sample: &WeatherForecastSample) -> f64 {
    const HORIZON_H: f64 = 48.0;
    const FLOOR: f64 = 0.2;
    let base_confidence = (1.0 - sample.age_h as f64 / HORIZON_H).clamp(FLOOR, 1.0);
    base_confidence * (1.0 - sample.irradiance_variability.unwrap_or(1.0))
}

/// The overall scalar of a series: the mean of its per-hour confidences, 0 for an empty one.
pub fn mean_slot_confidence(samples: &[WeatherForecastSample]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    samples.iter().map(slot_confidence).sum::<f64>() / samples.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::weather::SkyCondition;
    use chrono::{TimeZone, Utc};

    fn weather_sample(age_h: u32, irradiance_variability: Option<f64>) -> WeatherForecastSample {
        WeatherForecastSample {
            valid_at: Utc.with_ymd_and_hms(2026, 6, 21, 0, 0, 0).unwrap()
                + chrono::Duration::hours(age_h as i64),
            age_h,
            temperature_c: 20.0,
            ghi_w_m2: 400.0,
            wind_speed_kmh: None,
            rain_prob_pct: None,
            new_snowfall_cm: None,
            sky_condition: Some(SkyCondition::Clear),
            irradiance_variability,
        }
    }

    #[test]
    fn slot_confidence_uniform_sky_is_not_reduced() {
        let sample = weather_sample(1, Some(0.0));
        let base = (1.0 - 1.0 / 48.0_f64).clamp(0.2, 1.0);
        assert!((slot_confidence(&sample) - base).abs() < 1e-9);
    }

    #[test]
    fn slot_confidence_broken_sky_is_maximally_reduced() {
        assert_eq!(slot_confidence(&weather_sample(1, Some(1.0))), 0.0);
    }

    #[test]
    fn slot_confidence_missing_variability_treated_as_maximal_uncertainty() {
        assert_eq!(slot_confidence(&weather_sample(1, None)), 0.0);
    }

    #[test]
    fn slot_confidence_decays_with_age_h() {
        let near = weather_sample(1, Some(0.0));
        let far = weather_sample(40, Some(0.0));
        assert!(slot_confidence(&near) > slot_confidence(&far));
    }

    #[test]
    fn mean_slot_confidence_averages_the_hours_and_is_zero_for_an_empty_series() {
        let a = weather_sample(1, Some(0.0));
        let b = weather_sample(1, Some(1.0));
        let mean = mean_slot_confidence(&[a.clone(), b]);
        assert!((mean - slot_confidence(&a) / 2.0).abs() < 1e-9);
        assert_eq!(mean_slot_confidence(&[]), 0.0);
    }
}
