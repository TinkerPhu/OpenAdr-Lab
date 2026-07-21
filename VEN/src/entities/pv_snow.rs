//! Snow-cover state model for weather-sourced PV forecasting — a near-binary
//! self-clearing state machine, not a continuous melt-depth integrator. See
//! `docs/plans/weather-forecast-plugin.md` ("Snow cover") for the physical
//! reasoning. Pure state transition + fold, no I/O.
//!
//! Consumed by `entities::solar::weather_pv_forecast_series`, used by both
//! `GET /weather` (read-only diagnostic) and the planner's own PV input
//! (R-50, `entities::solar::resolve_weather_pv_kw`).

use crate::entities::asset_params::PvSnowParams;
use crate::entities::weather::WeatherForecastSample;

/// Whether the panel is currently assumed snow-covered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PvSnowState {
    pub covered: bool,
}

impl PvSnowState {
    /// Pure state transition for one forecast hour: (old_state, inputs) → new_state.
    pub fn step(self, params: &PvSnowParams, sample: &WeatherForecastSample) -> Self {
        let snowed = sample.new_snowfall_cm.unwrap_or(0.0) >= params.snowfall_trigger_cm;
        let melts = sample.temperature_c >= params.clear_threshold_c;
        Self {
            covered: snowed || (self.covered && !melts),
        }
    }
}

/// Run the snow-cover state machine forward over a forecast sequence,
/// starting from a known/assumed `initial` state. Pure fold — the planner
/// needs the whole horizon's trajectory in one shot, not a live tick-by-tick
/// simulation.
///
/// The "forecast-only fallback" for `initial` (used when no live PV
/// telemetry cross-check is available — see the design doc's "open
/// problem" section) is a calling convention, not separate code: pass
/// `PvSnowState::default()` (assume uncovered) and include the forecast's
/// own `age_h=0` "fact" sample as the first element of `samples` — see
/// `trajectory_bootstraps_from_forecasts_own_fact_sample` below.
pub fn snow_coverage_trajectory(
    initial: PvSnowState,
    params: &PvSnowParams,
    samples: &[WeatherForecastSample],
) -> Vec<PvSnowState> {
    let mut state = initial;
    samples
        .iter()
        .map(|s| {
            state = state.step(params, s);
            state
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn sample(new_snowfall_cm: Option<f64>, temperature_c: f64) -> WeatherForecastSample {
        sample_with_age(new_snowfall_cm, temperature_c, 1)
    }

    fn sample_with_age(
        new_snowfall_cm: Option<f64>,
        temperature_c: f64,
        age_h: u32,
    ) -> WeatherForecastSample {
        WeatherForecastSample {
            valid_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            age_h,
            temperature_c,
            ghi_w_m2: 0.0,
            wind_speed_kmh: None,
            rain_prob_pct: None,
            new_snowfall_cm,
            sky_condition: None,
            irradiance_variability: None,
        }
    }

    #[test]
    fn fresh_snowfall_above_trigger_sets_covered_regardless_of_prior_state() {
        let params = PvSnowParams::default();
        let uncovered = PvSnowState::default();
        let after = uncovered.step(&params, &sample(Some(1.0), -2.0));
        assert!(after.covered);
    }

    #[test]
    fn temperature_at_or_above_clear_threshold_clears_covered_state() {
        let params = PvSnowParams::default();
        let covered = PvSnowState { covered: true };
        let after = covered.step(&params, &sample(None, 2.0)); // >= 1.5 threshold
        assert!(!after.covered);
    }

    #[test]
    fn sustained_cold_with_no_new_snowfall_holds_covered_state() {
        let params = PvSnowParams::default();
        let covered = PvSnowState { covered: true };
        let after = covered.step(&params, &sample(None, -5.0));
        assert!(after.covered);
    }

    #[test]
    fn light_snowfall_below_trigger_does_not_cover_an_uncovered_panel() {
        let params = PvSnowParams::default();
        let uncovered = PvSnowState::default();
        let after = uncovered.step(&params, &sample(Some(0.05), -2.0)); // below 0.2 trigger
        assert!(!after.covered);
    }

    #[test]
    fn trajectory_snow_then_cold_then_warm_matches_expected_sequence() {
        let params = PvSnowParams::default();
        let samples = vec![
            sample(Some(1.0), -3.0), // snow falls → covered
            sample(None, -2.0),      // stays cold → covered
            sample(None, -1.0),      // stays cold → covered
            sample(None, 2.0),       // warms up → clears
        ];
        let traj = snow_coverage_trajectory(PvSnowState::default(), &params, &samples);
        assert_eq!(
            traj,
            vec![
                PvSnowState { covered: true },
                PvSnowState { covered: true },
                PvSnowState { covered: true },
                PvSnowState { covered: false },
            ]
        );
    }

    /// Demonstrates the "forecast-only fallback" calling convention: no live
    /// PV telemetry cross-check available, so `initial` is the conservative
    /// `PvSnowState::default()` (assume uncovered) and the forecast's own
    /// `age_h=0` "fact" sample is included as the first element — if it
    /// already snowed within that most-recent hour, coverage begins there
    /// rather than waiting for the first *forward* forecast sample.
    #[test]
    fn trajectory_bootstraps_from_forecasts_own_fact_sample() {
        let params = PvSnowParams::default();
        let samples = vec![
            sample_with_age(Some(2.0), -4.0, 0), // age_h=0: it already snowed this past hour
            sample_with_age(None, -3.0, 1),      // age_h=1: still cold → stays covered
        ];
        let traj = snow_coverage_trajectory(PvSnowState::default(), &params, &samples);
        assert_eq!(
            traj,
            vec![PvSnowState { covered: true }, PvSnowState { covered: true }],
            "coverage must begin at the fact sample (age_h=0), not only from forward forecasts"
        );
    }
}
