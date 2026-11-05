//! The snow-cover conclusion of one PV array (R-55): what the live measurement says about the panel,
//! checked against the snow-free weather forecast.
//!
//! Owns: the one function that turns (previous state, weather series, measurement) into the new state;
//! the VEN's PV asset and the forecaster both call it. Reads: values handed in. Must never: read a clock
//! (callers pass `now`), or keep state of its own.

use chrono::{DateTime, Utc};

use crate::pv_snow::{PvSnowState, SnowEvidence};
use crate::solar::{weather_snow_reference_at, WeatherPvForecastSlot};

/// What one array can be checked against right now.
#[derive(Debug, Clone, Copy)]
pub struct SnowObservationInput<'a> {
    /// The weather-sourced PV series of the array (with its snow-free twin per slot).
    pub series: &'a [WeatherPvForecastSlot],
    pub now: DateTime<Utc>,
    /// The array's real measured output, when there is a fresh one.
    pub measured_kw: Option<f64>,
    /// A generation limit in force: it lowers what the array is expected to deliver, so a curtailed
    /// array is not read as a covered one.
    pub generation_limit_kw: Option<f64>,
    /// The array's rating, the scale below which a dim hour says nothing about snow.
    pub rated_kw: f64,
}

/// The state after cross-checking the measurement against the snow-free forecast. Silent (state
/// unchanged) when there is nothing to compare: no series, no measurement, or conditions too dim to tell.
/// Warm air clears a covered panel even without a measurement, the forecast's own melt rule applied to now.
pub fn observe_snow(state: PvSnowState, input: SnowObservationInput<'_>) -> PvSnowState {
    let Some((snow_free_kw, snow_possible)) = weather_snow_reference_at(input.series, input.now) else {
        return state;
    };
    // Warm air melts a covered panel: the forecast's own melt rule, applied to now, so a state concluded
    // in the cold cannot outlive the cold when the measurement is gone.
    let state = if snow_possible { state } else { PvSnowState { covered: false } };
    let Some(measured_kw) = input.measured_kw else {
        return state;
    };
    let expected_kw = input.generation_limit_kw.map_or(snow_free_kw, |limit_kw| snow_free_kw.min(limit_kw));
    state.observed(SnowEvidence { expected_kw, measured_kw, snow_possible }, input.rated_kw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn noon() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 15, 12, 0, 0).unwrap()
    }

    /// A one-slot series at noon: the snow-free output and whether the air is cold enough for snow.
    fn series(snow_free_kw: f64, snow_possible: bool) -> Vec<WeatherPvForecastSlot> {
        vec![WeatherPvForecastSlot {
            valid_at: noon(),
            forecast_ac_kw: snow_free_kw,
            snow_covered: false,
            snow_free_ac_kw: snow_free_kw,
            snow_possible,
        }]
    }

    fn input(series: &[WeatherPvForecastSlot], measured_kw: Option<f64>) -> SnowObservationInput<'_> {
        SnowObservationInput { series, now: noon(), measured_kw, generation_limit_kw: None, rated_kw: 10.0 }
    }

    const COVERED: PvSnowState = PvSnowState { covered: true };
    const CLEAR: PvSnowState = PvSnowState { covered: false };

    #[test]
    fn observe_snow_concludes_covered_when_the_measurement_is_far_below_the_snow_free_forecast() {
        let s = series(6.0, true);
        assert_eq!(observe_snow(CLEAR, input(&s, Some(0.1))), COVERED);
    }

    #[test]
    fn observe_snow_clears_the_state_when_the_array_delivers_what_the_weather_predicts() {
        let s = series(6.0, true);
        assert_eq!(observe_snow(COVERED, input(&s, Some(5.5))), CLEAR);
    }

    #[test]
    fn observe_snow_without_a_measurement_keeps_a_covered_state_while_the_air_stays_cold() {
        let s = series(6.0, true);
        assert_eq!(observe_snow(COVERED, input(&s, None)), COVERED);
    }

    #[test]
    fn observe_snow_without_a_measurement_clears_a_covered_state_when_the_air_is_warm() {
        let s = series(6.0, false);
        assert_eq!(observe_snow(COVERED, input(&s, None)), CLEAR);
    }

    #[test]
    fn observe_snow_with_no_series_changes_nothing() {
        assert_eq!(observe_snow(COVERED, input(&[], Some(0.0))), COVERED);
        assert_eq!(observe_snow(CLEAR, input(&[], Some(0.0))), CLEAR);
    }

    #[test]
    fn observe_snow_does_not_read_a_curtailed_array_as_covered() {
        let s = series(6.0, true);
        let mut i = input(&s, Some(0.1));
        i.generation_limit_kw = Some(0.1);
        assert_eq!(observe_snow(CLEAR, i), CLEAR);
    }

    #[test]
    fn observe_snow_in_a_dim_hour_says_nothing() {
        let s = series(0.2, true);
        assert_eq!(observe_snow(CLEAR, input(&s, Some(0.0))), CLEAR);
    }
}
