//! Snow-cover state model for weather-sourced PV forecasting — a near-binary
//! self-clearing state machine, not a continuous melt-depth integrator. See
//! `docs/architecture/weather_forecast.md` ("Snow cover") for the physical
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

/// Below this share of the array's rating, the expected (snow-free) output is too small to tell a
/// covered panel from a dim hour, so a reading says nothing about snow.
const OBSERVE_MIN_EXPECTED_FRACTION_OF_RATED: f64 = 0.1;
/// A panel delivering at most this share of what the weather says it could is covered.
const COVERED_AT_OR_BELOW_RATIO: f64 = 0.15;
/// A panel delivering at least this share of what the weather says it could is clear. Between the
/// two ratios nothing is concluded (cloud, partial cover): the state stays as it was.
const CLEAR_AT_OR_ABOVE_RATIO: f64 = 0.5;

/// What the live inputs say about the panel right now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnowEvidence {
    /// Output the weather predicts for this instant with no snow on the panel, after any
    /// generation limit in force (a curtailed array is not a covered one).
    pub expected_kw: f64,
    /// What the array actually delivered, from the real measurement.
    pub measured_kw: f64,
    /// Whether the air is cold enough for snow to lie at all (`temperature < clear_threshold_c`).
    /// A shortfall in warm air is cloud, shading or a fault, not snow.
    pub snow_possible: bool,
}

impl PvSnowState {
    /// The panel's state after looking at what it actually produces, starting from `self`: the
    /// telemetry cross-check R-55 describes. Replaces the forecast-only guess as the starting
    /// state of the forecast trajectory, so a panel that is covered *now* is forecast covered
    /// until the melt condition, however sunny the forecast.
    pub fn observed(self, evidence: SnowEvidence, rated_kw: f64) -> Self {
        let SnowEvidence {
            expected_kw,
            measured_kw,
            snow_possible,
        } = evidence;
        if rated_kw <= 0.0 || expected_kw < OBSERVE_MIN_EXPECTED_FRACTION_OF_RATED * rated_kw {
            return self;
        }
        let ratio = measured_kw / expected_kw;
        if ratio <= COVERED_AT_OR_BELOW_RATIO && snow_possible {
            Self { covered: true }
        } else if ratio >= CLEAR_AT_OR_ABOVE_RATIO {
            Self { covered: false }
        } else {
            self
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

    fn evidence(expected_kw: f64, measured_kw: f64, snow_possible: bool) -> SnowEvidence {
        SnowEvidence {
            expected_kw,
            measured_kw,
            snow_possible,
        }
    }

    #[test]
    fn producing_next_to_nothing_in_bright_cold_conditions_means_covered() {
        let after = PvSnowState::default().observed(evidence(4.0, 0.1, true), 5.0);
        assert!(after.covered);
    }

    #[test]
    fn producing_next_to_nothing_in_warm_air_is_not_snow() {
        let after = PvSnowState::default().observed(evidence(4.0, 0.0, false), 5.0);
        assert!(!after.covered);
        let still = PvSnowState { covered: true }.observed(evidence(4.0, 0.0, false), 5.0);
        assert!(
            still.covered,
            "no evidence either way: the state stays as it was"
        );
    }

    #[test]
    fn producing_what_the_weather_predicts_clears_a_covered_state() {
        let after = PvSnowState { covered: true }.observed(evidence(4.0, 3.8, true), 5.0);
        assert!(!after.covered);
    }

    #[test]
    fn a_dim_hour_says_nothing_about_snow() {
        // expected 0.2 kW on a 5 kW array is below the 10 % floor
        let after = PvSnowState::default().observed(evidence(0.2, 0.0, true), 5.0);
        assert!(!after.covered);
        let covered = PvSnowState { covered: true }.observed(evidence(0.2, 0.2, true), 5.0);
        assert!(covered.covered);
    }

    #[test]
    fn a_partial_shortfall_leaves_the_state_unchanged() {
        for covered in [false, true] {
            let before = PvSnowState { covered };
            assert_eq!(before.observed(evidence(4.0, 1.2, true), 5.0), before); // ratio 0.3
        }
    }

    #[test]
    fn an_array_without_a_rating_cannot_be_observed() {
        let before = PvSnowState { covered: true };
        assert_eq!(before.observed(evidence(4.0, 4.0, true), 0.0), before);
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
