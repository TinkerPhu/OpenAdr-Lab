//! The base-load side of the forecaster: collects the measured house load and turns it into a 48-hour series.
//!
//! Owns: the sample buckets (10-minute means, kept for the learner's window), the call of
//! `lab_forecast::base_load_learner` (the one learner the VEN also uses) and the cold-start series
//! published while there is too little history. Reads: readings and `now` handed in. Must never: read a
//! clock or a file (`cache` does the file), or compute a profile itself.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, TimeZone, Utc};
use lab_forecast::base_load_learner::{learn_profile, LearnConfig, LearnedProfile, LoadSample};
use lab_forecast::measurement_wire::MeasurementReading;

/// One bucket is the mean of the readings of this many seconds.
const BUCKET_S: i64 = 600;
/// Hours published ahead.
pub const HORIZON_H: usize = 48;
/// Flat consumption assumed while there is no history at all (kW).
pub const COLD_START_DEFAULT_KW: f64 = 0.4;
/// Confidence of a learned series and of a cold-start one (placeholders until accuracy data exists, as in
/// the VEN's heuristic forecast).
const LEARNED_CONFIDENCE: f64 = 0.5;
const COLD_START_CONFIDENCE: f64 = 0.2;
/// `model_version` of a learned series and of a cold-start one.
pub const MODEL_LEARNED: &str = "base-load-profile-1";
pub const MODEL_COLD_START: &str = "base-load-cold-start-1";

/// The series to publish: average kW per hour from `start`.
#[derive(Debug, Clone, PartialEq)]
pub struct BaseLoadSeries {
    pub start: DateTime<Utc>,
    pub values_kw: Vec<f64>,
    pub model_version: &'static str,
    pub confidence: f64,
    pub cold_start: bool,
}

pub struct BaseLoadModel {
    cfg: LearnConfig,
    /// bucket index (seconds since epoch / BUCKET_S) -> (sum of kW, readings)
    buckets: BTreeMap<i64, (f64, u32)>,
    learned: Option<LearnedProfile>,
    last_reading_at: Option<DateTime<Utc>>,
}

impl BaseLoadModel {
    pub fn new(cfg: LearnConfig) -> Self {
        Self {
            cfg,
            buckets: BTreeMap::new(),
            learned: None,
            last_reading_at: None,
        }
    }

    /// Take one measured reading (kW, positive = consumption) into its bucket; buckets older than the
    /// learner's window are dropped.
    pub fn record(&mut self, reading: MeasurementReading) {
        let (kw, ts) = reading;
        self.add(ts, kw);
        if self.last_reading_at.is_none_or(|last| ts > last) {
            self.last_reading_at = Some(ts);
        }
        let oldest = bucket_of(ts) - i64::from(self.cfg.rolling_window_days) * 86_400 / BUCKET_S;
        self.buckets = self.buckets.split_off(&oldest);
    }

    /// The time of the newest reading, for the status.
    pub fn last_reading_at(&self) -> Option<DateTime<Utc>> {
        self.last_reading_at
    }

    /// The buckets as samples (one per bucket, at its start), oldest first: what is saved to the cache.
    pub fn history(&self) -> Vec<LoadSample> {
        self.buckets
            .iter()
            .map(|(bucket, (sum, n))| LoadSample {
                ts: Utc
                    .timestamp_opt(bucket * BUCKET_S, 0)
                    .single()
                    .expect("a bucket start is a valid time"),
                power_kw: sum / f64::from(*n),
            })
            .collect()
    }

    /// Put saved samples back (a restart); they land in their buckets like live readings.
    pub fn restore(&mut self, samples: &[LoadSample]) {
        for s in samples {
            self.add(s.ts, s.power_kw);
        }
        if let Some(newest) = samples.iter().map(|s| s.ts).max() {
            self.buckets = self.buckets.split_off(
                &(bucket_of(newest) - i64::from(self.cfg.rolling_window_days) * 86_400 / BUCKET_S),
            );
        }
    }

    /// The series for the next 48 hours at `now`: learned when the history clears the learner's gate,
    /// else the flat cold-start series (the mean of what exists, or the default).
    pub fn series(&mut self, now: DateTime<Utc>) -> BaseLoadSeries {
        let samples = self.history();
        let start = hour_start(now);
        if let Some(profile) = learn_profile(&samples, now, &self.cfg, self.learned.as_ref()) {
            let values_kw = (0..HORIZON_H)
                .map(|i| {
                    profile
                        .sample_kw(start + Duration::hours(i as i64))
                        .max(0.0)
                })
                .collect();
            self.learned = Some(profile);
            return BaseLoadSeries {
                start,
                values_kw,
                model_version: MODEL_LEARNED,
                confidence: LEARNED_CONFIDENCE,
                cold_start: false,
            };
        }
        let flat_kw = if samples.is_empty() {
            COLD_START_DEFAULT_KW
        } else {
            samples.iter().map(|s| s.power_kw).sum::<f64>() / samples.len() as f64
        };
        BaseLoadSeries {
            start,
            values_kw: vec![flat_kw; HORIZON_H],
            model_version: MODEL_COLD_START,
            confidence: COLD_START_CONFIDENCE,
            cold_start: true,
        }
    }

    fn add(&mut self, ts: DateTime<Utc>, kw: f64) {
        let entry = self.buckets.entry(bucket_of(ts)).or_insert((0.0, 0));
        entry.0 += kw;
        entry.1 += 1;
    }
}

fn bucket_of(ts: DateTime<Utc>) -> i64 {
    ts.timestamp().div_euclid(BUCKET_S)
}

fn hour_start(t: DateTime<Utc>) -> DateTime<Utc> {
    Utc.timestamp_opt(t.timestamp().div_euclid(3600) * 3600, 0)
        .single()
        .expect("an hour start is a valid time")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, TimeZone, Timelike};

    fn at(d: u32, h: u32, m: u32) -> DateTime<Utc> {
        // 2026-07-14 is a Tuesday.
        Utc.with_ymd_and_hms(2026, 7, d, h, m, 0).unwrap()
    }

    fn model() -> BaseLoadModel {
        BaseLoadModel::new(LearnConfig::default())
    }

    /// One reading a minute for `days` ending at `end`, with power from `f`.
    fn feed(
        m: &mut BaseLoadModel,
        end: DateTime<Utc>,
        days: i64,
        f: impl Fn(DateTime<Utc>) -> f64,
    ) {
        let mut ts = end - Duration::days(days);
        while ts < end {
            m.record((f(ts), ts));
            ts += Duration::minutes(1);
        }
    }

    #[test]
    fn record_averages_the_readings_of_one_bucket() {
        let mut m = model();
        m.record((1.0, at(14, 10, 0)));
        m.record((3.0, at(14, 10, 5)));
        let h = m.history();
        assert_eq!(h.len(), 1);
        assert_eq!(
            h[0],
            LoadSample {
                ts: at(14, 10, 0),
                power_kw: 2.0
            }
        );
        assert_eq!(m.last_reading_at(), Some(at(14, 10, 5)));
    }

    #[test]
    fn record_drops_buckets_older_than_the_learner_window() {
        let mut m = model();
        m.record((1.0, at(14, 10, 0) - Duration::days(60)));
        m.record((1.0, at(14, 10, 0)));
        assert_eq!(
            m.history().len(),
            1,
            "a 60-day-old bucket is outside the 56-day window"
        );
    }

    #[test]
    fn history_is_oldest_first_and_restore_round_trips_it() {
        let mut a = model();
        feed(&mut a, at(14, 12, 0), 2, |_| 0.5);
        let saved = a.history();
        assert!(saved.windows(2).all(|w| w[0].ts < w[1].ts));
        let mut b = model();
        b.restore(&saved);
        assert_eq!(b.history(), saved);
    }

    #[test]
    fn series_without_history_is_the_flat_cold_start_default() {
        let s = model().series(at(14, 12, 30));
        assert!(s.cold_start);
        assert_eq!(s.model_version, MODEL_COLD_START);
        assert_eq!(s.values_kw, vec![COLD_START_DEFAULT_KW; HORIZON_H]);
        assert_eq!(
            s.start,
            at(14, 12, 0),
            "the first step is the hour containing now"
        );
    }

    #[test]
    fn series_with_a_little_history_is_flat_at_its_mean_and_still_cold_start() {
        let mut few = model_with_buckets(50, 0.8);
        let s = few.series(at(14, 12, 30));
        assert!(s.cold_start);
        assert!(s.values_kw.iter().all(|v| (*v - 0.8).abs() < 1e-9));
    }

    fn model_with_buckets(n: i64, kw: f64) -> BaseLoadModel {
        let mut m = model();
        for i in 0..n {
            m.record((kw, at(14, 0, 0) + Duration::seconds(i * BUCKET_S)));
        }
        m
    }

    #[test]
    fn series_with_enough_history_is_learned_and_follows_the_weekday_hour() {
        let mut m = model();
        let now = at(14, 12, 30);
        feed(&mut m, now, 28, |ts| {
            let weekday = ts.weekday().num_days_from_monday() < 5;
            0.3 + if weekday && ts.hour() == 8 { 1.2 } else { 0.0 }
        });
        let s = m.series(now);
        assert!(!s.cold_start);
        assert_eq!(s.model_version, MODEL_LEARNED);
        assert_eq!(s.values_kw.len(), HORIZON_H);
        // start = 12:00 Tuesday; index 20 = Wednesday 08:00 (a weekday peak), index 15 = Wednesday 03:00.
        let wednesday_8 = s.values_kw[20];
        let wednesday_3 = s.values_kw[15];
        assert!(
            wednesday_8 > wednesday_3 + 0.7,
            "{wednesday_8} vs {wednesday_3}"
        );
    }

    #[test]
    fn series_values_are_finite_and_not_negative_for_a_normal_load() {
        let mut m = model();
        feed(&mut m, at(14, 12, 30), 14, |_| 0.4);
        let s = m.series(at(14, 12, 30));
        assert!(s.values_kw.iter().all(|v| v.is_finite() && *v >= 0.0));
        assert!((0.0..=1.0).contains(&s.confidence));
    }
}
