//! A PV forecast received from the forecaster, as the VEN uses it: which series to plan on and where it came from.
//!
//! Owns: the one decision "received series or own computation" (`choose_pv_series`), the translation of a
//! payload to the slot series the PV asset takes, and the source report the VEN shows. Reads: the payload,
//! `now`, and a closure for the VEN's own computation (called only when needed: it is the expensive physics).
//! Must never: read a clock or interpret the series itself: the PV asset stays the only interpreter.

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use ts_rs::TS;

use crate::forecast_payload::{ForecastPayload, Sign};
use crate::solar::WeatherPvForecastSlot;

/// Where the series a consumer plans on came from.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SeriesSource {
    /// The forecaster container's retained message.
    Forecaster {
        model_version: String,
        generated_at: DateTime<Utc>,
    },
    /// The consumer's own computation from the weather forecast.
    InProcess,
}

/// A received consumption series (base load) as a value-at-time lookup: step-held (each value covers its
/// whole step), `None` outside the series.
#[derive(Debug, Clone, PartialEq)]
pub struct ReceivedSeries {
    pub start: DateTime<Utc>,
    pub step_s: u32,
    pub values_kw: Vec<f64>,
}

impl ReceivedSeries {
    /// The series of a payload declared as consumption; `None` for any other declaration.
    pub fn from_consumption(payload: &ForecastPayload) -> Option<Self> {
        (payload.sign == Sign::ConsumptionPositive).then(|| Self {
            start: payload.start,
            step_s: payload.step_s,
            values_kw: payload.values.clone(),
        })
    }

    /// The value of the step containing `t`.
    pub fn at(&self, t: DateTime<Utc>) -> Option<f64> {
        let elapsed_s = (t - self.start).num_seconds();
        if elapsed_s < 0 {
            return None;
        }
        let index = usize::try_from(elapsed_s / i64::from(self.step_s)).ok()?;
        self.values_kw.get(index).copied()
    }
}

/// The base-load series to plan on when the received one is usable at `now`.
pub fn usable_base_load(
    payload: Option<&ForecastPayload>,
    now: DateTime<Utc>,
) -> Option<ReceivedSeries> {
    payload
        .filter(|p| p.is_fresh(now))
        .and_then(ReceivedSeries::from_consumption)
}

/// The slot series of a received PV payload, one slot per step. The forecaster already applied the snow
/// state, so each slot carries the received value as its own snow-free reference and `snow_possible` is
/// false: the asset's snow observation then has nothing to add and cannot contradict the series.
pub fn pv_slots(payload: &ForecastPayload) -> Vec<WeatherPvForecastSlot> {
    let step = Duration::seconds(i64::from(payload.step_s));
    payload
        .values
        .iter()
        .enumerate()
        .map(|(i, kw)| WeatherPvForecastSlot {
            valid_at: payload.start + step * i as i32,
            forecast_ac_kw: *kw,
            snow_covered: false,
            snow_free_ac_kw: *kw,
            snow_possible: false,
        })
        .collect()
}

/// The payload when it can be planned on at `now`: before its `valid_until`, and declared as generation.
pub fn usable_pv(
    payload: Option<&ForecastPayload>,
    now: DateTime<Utc>,
) -> Option<&ForecastPayload> {
    payload.filter(|p| p.sign == Sign::GenerationPositive && p.is_fresh(now))
}

/// The series to plan on: the received one while it is usable, else the consumer's own (`own`, called
/// only then). `None` when neither exists.
pub fn choose_pv_series(
    received: Option<&ForecastPayload>,
    now: DateTime<Utc>,
    own: impl FnOnce() -> Option<Vec<WeatherPvForecastSlot>>,
) -> Option<(Vec<WeatherPvForecastSlot>, SeriesSource)> {
    match usable_pv(received, now) {
        Some(p) => Some((
            pv_slots(p),
            SeriesSource::Forecaster {
                model_version: p.model_version.clone(),
                generated_at: p.generated_at,
            },
        )),
        None => own().map(|series| (series, SeriesSource::InProcess)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forecast_payload::{PayloadMeta, SeriesGrid};
    use chrono::TimeZone;
    use std::cell::Cell;

    fn at(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 5, h, m, 0).unwrap()
    }

    fn pv_payload() -> ForecastPayload {
        ForecastPayload::pv_natural(
            PayloadMeta {
                generated_at: at(10, 0),
                valid_until: at(11, 30),
                model_version: "pv-weather-1".into(),
                producer_id: "fc".into(),
                confidence: 0.7,
            },
            SeriesGrid {
                start: at(9, 0),
                step_s: 3600,
            },
            vec![0.0, 1.5, 3.0, 2.0],
        )
        .unwrap()
    }

    fn own_series() -> Vec<WeatherPvForecastSlot> {
        vec![WeatherPvForecastSlot {
            valid_at: at(9, 0),
            forecast_ac_kw: 9.9,
            snow_covered: false,
            snow_free_ac_kw: 9.9,
            snow_possible: false,
        }]
    }

    #[test]
    fn pv_slots_has_one_slot_per_step_at_the_step_times() {
        let slots = pv_slots(&pv_payload());
        assert_eq!(slots.len(), 4);
        assert_eq!(slots[0].valid_at, at(9, 0));
        assert_eq!(slots[2].valid_at, at(11, 0));
        assert_eq!(slots[2].forecast_ac_kw, 3.0);
    }

    #[test]
    fn pv_slots_leaves_nothing_for_the_assets_snow_observation_to_contradict() {
        for s in pv_slots(&pv_payload()) {
            assert_eq!(s.snow_free_ac_kw, s.forecast_ac_kw);
            assert!(!s.snow_possible && !s.snow_covered);
        }
    }

    #[test]
    fn usable_pv_is_the_payload_until_valid_until_inclusive_and_none_after() {
        let p = pv_payload();
        assert!(usable_pv(Some(&p), at(11, 30)).is_some());
        assert!(usable_pv(Some(&p), at(11, 31)).is_none());
        assert!(usable_pv(None, at(10, 0)).is_none());
    }

    #[test]
    fn usable_pv_refuses_a_series_not_declared_as_generation() {
        let mut p = pv_payload();
        p.sign = Sign::ConsumptionPositive;
        assert!(usable_pv(Some(&p), at(10, 0)).is_none());
    }

    #[test]
    fn choose_pv_series_prefers_the_received_one_and_does_not_compute_its_own() {
        let p = pv_payload();
        let called = Cell::new(false);
        let (series, source) = choose_pv_series(Some(&p), at(10, 5), || {
            called.set(true);
            Some(own_series())
        })
        .unwrap();
        assert!(
            !called.get(),
            "the physics is skipped when the received series serves"
        );
        assert_eq!(series.len(), 4);
        assert_eq!(
            source,
            SeriesSource::Forecaster {
                model_version: "pv-weather-1".into(),
                generated_at: at(10, 0)
            }
        );
    }

    #[test]
    fn choose_pv_series_falls_back_to_its_own_when_the_received_one_is_stale_or_missing() {
        let p = pv_payload();
        for received in [Some(&p), None] {
            let (series, source) =
                choose_pv_series(received, at(12, 0), || Some(own_series())).unwrap();
            assert_eq!(series[0].forecast_ac_kw, 9.9);
            assert_eq!(source, SeriesSource::InProcess);
        }
    }

    fn load_payload() -> ForecastPayload {
        ForecastPayload::base_load(
            PayloadMeta {
                generated_at: at(10, 0),
                valid_until: at(11, 30),
                model_version: "base-load-profile-1".into(),
                producer_id: "fc".into(),
                confidence: 0.5,
            },
            SeriesGrid {
                start: at(10, 0),
                step_s: 3600,
            },
            vec![0.4, 0.6, 0.8],
        )
        .unwrap()
    }

    #[test]
    fn received_series_holds_each_value_over_its_step_and_is_none_outside() {
        let s = ReceivedSeries::from_consumption(&load_payload()).unwrap();
        assert_eq!(s.at(at(10, 0)), Some(0.4));
        assert_eq!(s.at(at(10, 59)), Some(0.4));
        assert_eq!(s.at(at(11, 0)), Some(0.6));
        assert_eq!(s.at(at(12, 59)), Some(0.8));
        assert_eq!(s.at(at(13, 0)), None, "after the last step");
        assert_eq!(s.at(at(9, 59)), None, "before the first step");
    }

    #[test]
    fn received_series_refuses_a_payload_declared_as_generation() {
        assert!(ReceivedSeries::from_consumption(&pv_payload()).is_none());
    }

    #[test]
    fn usable_base_load_follows_valid_until() {
        let p = load_payload();
        assert!(usable_base_load(Some(&p), at(11, 30)).is_some());
        assert!(usable_base_load(Some(&p), at(11, 31)).is_none());
        assert!(usable_base_load(None, at(10, 0)).is_none());
    }

    #[test]
    fn choose_pv_series_is_none_when_nothing_is_available() {
        assert!(choose_pv_series(None, at(10, 0), || None).is_none());
    }
}
