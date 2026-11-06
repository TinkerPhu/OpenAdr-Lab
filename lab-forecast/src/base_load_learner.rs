//! The base-load learner: a typical-day profile per day of the week, learned from measured power samples.
//!
//! Owns: the one implementation of "what does this site's unmetered consumption look like by weekday and
//! hour" (EWMA-weighted hourly means shrunk toward the overall mean, plus a seasonal factor), shared by the
//! VEN (which feeds it ticks from its history store) and the forecaster (which feeds it its own cache).
//! Reads: the samples and `now` handed in. Must never: read a clock, a store or a file.

use chrono::{DateTime, Datelike, Duration, Timelike, Utc};

/// One measured power value (kW, positive = consumption).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoadSample {
    pub ts: DateTime<Utc>,
    pub power_kw: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct LearnConfig {
    /// How far back samples count. Kept well under ~91 days (one season): the 30-day-vs-window
    /// `seasonal_factor` split assumes the window is roughly one season.
    pub rolling_window_days: u32,
    /// EWMA half-life: a sample this many days old weighs half of one from `now`.
    pub ewma_halflife_days: f64,
    /// Shrinkage strength in day-equivalents of data: a weekday/hour cell with this much data is blended
    /// 50/50 toward the flat overall mean; far more and it is trusted on its own, far less it leans on the mean.
    pub shrinkage_k_days: f64,
    /// Cold-start gate: fewer samples than this and no profile is produced.
    pub min_samples_for_confidence: usize,
}

impl Default for LearnConfig {
    fn default() -> Self {
        Self {
            rolling_window_days: 56,
            ewma_halflife_days: 28.0,
            shrinkage_k_days: 2.5,
            min_samples_for_confidence: 100,
        }
    }
}

/// A learned typical day: one 24-hour curve per weekday (`[0]` = Monday .. `[6]` = Sunday).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LearnedProfile {
    pub daytime_profile_kw: [Vec<f64>; 7],
    /// Multiplier for the current season (1.2 = 20% more than the window's mean).
    pub seasonal_factor: f64,
    pub last_updated: Option<DateTime<Utc>>,
    /// Recency-weighted mean absolute error of the previous profile against this window's samples;
    /// `None` on the first run.
    pub recent_mean_abs_error_kw: Option<f64>,
}

/// `profile[weekday][hour] x seasonal_factor` at `t`; 0 where the curve has no entry. The one sampling
/// formula: the VEN's `AssetHeuristics` and `LearnedProfile` both call it.
pub fn sample_profile_kw(profile: &[Vec<f64>; 7], seasonal_factor: f64, t: DateTime<Utc>) -> f64 {
    let bucket = t.weekday().num_days_from_monday() as usize;
    let hour = t.hour() as usize;
    profile[bucket].get(hour).copied().unwrap_or(0.0) * seasonal_factor
}

impl LearnedProfile {
    pub fn sample_kw(&self, t: DateTime<Utc>) -> f64 {
        sample_profile_kw(&self.daytime_profile_kw, self.seasonal_factor, t)
    }
}

/// Blend weight of a cell's own mean against the flat overall mean: 0 with no data, 0.5 at
/// `effective_days == k_days`, towards 1 with much data.
pub fn shrinkage_blend(effective_days: f64, k_days: f64) -> f64 {
    effective_days / (effective_days + k_days)
}

/// Learn a profile from `samples` (any order; only those inside the window count). `None` below the
/// cold-start gate.
pub fn learn_profile(
    samples: &[LoadSample],
    now: DateTime<Utc>,
    cfg: &LearnConfig,
    previous: Option<&LearnedProfile>,
) -> Option<LearnedProfile> {
    let from = now - Duration::days(i64::from(cfg.rolling_window_days));
    let samples: Vec<&LoadSample> = samples
        .iter()
        .filter(|s| s.ts >= from && s.ts <= now)
        .collect();
    if samples.len() < cfg.min_samples_for_confidence {
        return None;
    }
    let weight = |ts: DateTime<Utc>| {
        ewma_weight(
            (now - ts).num_seconds() as f64 / 86_400.0,
            cfg.ewma_halflife_days,
        )
    };

    // Index = chrono::Weekday::num_days_from_monday(): [0]=Mon..[6]=Sun.
    let mut hour_sum = [[0.0_f64; 24]; 7];
    let mut hour_weight = [[0.0_f64; 24]; 7];
    let (mut overall_sum, mut overall_weight) = (0.0_f64, 0.0_f64);
    for s in &samples {
        let w = weight(s.ts);
        let (hour, bucket) = (
            s.ts.hour() as usize,
            s.ts.weekday().num_days_from_monday() as usize,
        );
        hour_sum[bucket][hour] += s.power_kw * w;
        hour_weight[bucket][hour] += w;
        overall_sum += s.power_kw * w;
        overall_weight += w;
    }
    let overall_mean = if overall_weight > 0.0 {
        overall_sum / overall_weight
    } else {
        0.0
    };

    // Self-calibrated from the data itself (samples per hour of window), so the sampling grain of the
    // caller does not matter.
    let avg_samples_per_hour = samples.len() as f64 / (f64::from(cfg.rolling_window_days) * 24.0);
    let daytime_profile_kw: [Vec<f64>; 7] = std::array::from_fn(|bucket| {
        (0..24)
            .map(|h| {
                let effective_days = hour_weight[bucket][h] / avg_samples_per_hour.max(1e-9);
                let blend = shrinkage_blend(effective_days, cfg.shrinkage_k_days);
                let cell_mean = if hour_weight[bucket][h] > 0.0 {
                    hour_sum[bucket][h] / hour_weight[bucket][h]
                } else {
                    0.0
                };
                blend * cell_mean + (1.0 - blend) * overall_mean
            })
            .collect()
    });

    // Rolling 30-day mean against the window's mean; near 1.0 with short history.
    let thirty_days_ago = now - Duration::days(30);
    let (recent_sum, recent_weight) = samples
        .iter()
        .filter(|s| s.ts >= thirty_days_ago)
        .fold((0.0_f64, 0.0_f64), |(sum, wt), s| {
            (sum + s.power_kw * weight(s.ts), wt + weight(s.ts))
        });
    let recent_mean = if recent_weight > 0.0 {
        recent_sum / recent_weight
    } else {
        overall_mean
    };
    let seasonal_factor = if overall_mean.abs() > 1e-9 {
        recent_mean / overall_mean
    } else {
        1.0
    };

    let recent_mean_abs_error_kw = previous.map(|prev| {
        let (mut err_sum, mut err_weight) = (0.0_f64, 0.0_f64);
        for s in &samples {
            let w = weight(s.ts);
            err_sum += (s.power_kw - prev.sample_kw(s.ts)).abs() * w;
            err_weight += w;
        }
        if err_weight > 0.0 {
            err_sum / err_weight
        } else {
            0.0
        }
    });

    Some(LearnedProfile {
        daytime_profile_kw,
        seasonal_factor,
        last_updated: Some(now),
        recent_mean_abs_error_kw,
    })
}

fn ewma_weight(age_days: f64, halflife_days: f64) -> f64 {
    0.5_f64.powf(age_days / halflife_days)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        // A Tuesday.
        Utc.with_ymd_and_hms(2026, 7, 14, 12, 0, 0).unwrap()
    }

    /// One sample per minute over `days`, with power from `f`.
    fn minutes(days: i64, f: impl Fn(DateTime<Utc>) -> f64) -> Vec<LoadSample> {
        let end = now();
        let mut ts = end - Duration::days(days);
        let mut out = Vec::new();
        while ts < end {
            out.push(LoadSample {
                ts,
                power_kw: f(ts),
            });
            ts += Duration::minutes(1);
        }
        out
    }

    /// 0.3 kW always, plus a 1.2 kW coffee peak at 08:00 on weekdays.
    fn weekday_coffee(ts: DateTime<Utc>) -> f64 {
        let weekday = ts.weekday().num_days_from_monday() < 5;
        0.3 + if weekday && ts.hour() == 8 { 1.2 } else { 0.0 }
    }

    #[test]
    fn learn_profile_below_the_cold_start_gate_returns_none() {
        assert!(learn_profile(&[], now(), &LearnConfig::default(), None).is_none());
        let few = minutes(1, |_| 0.3)[..99].to_vec();
        assert!(learn_profile(&few, now(), &LearnConfig::default(), None).is_none());
    }

    #[test]
    fn learn_profile_gives_24_hours_for_each_weekday() {
        let p = learn_profile(&minutes(14, |_| 0.3), now(), &LearnConfig::default(), None).unwrap();
        for day in &p.daytime_profile_kw {
            assert_eq!(day.len(), 24);
        }
        assert_eq!(p.last_updated, Some(now()));
    }

    #[test]
    fn learn_profile_converges_to_the_weekday_peak_and_a_quiet_weekend() {
        let p = learn_profile(
            &minutes(28, weekday_coffee),
            now(),
            &LearnConfig::default(),
            None,
        )
        .unwrap();
        let tuesday = &p.daytime_profile_kw[1];
        let saturday = &p.daytime_profile_kw[5];
        // Four Tuesdays of data: the cell is shrunk toward the overall mean by design, so the 1.5 kW peak
        // shows as clearly above the 0.3 kW baseline without reaching its full height.
        assert!(
            tuesday[8] > 1.0,
            "the weekday peak is learned: {}",
            tuesday[8]
        );
        assert!(tuesday[8] > saturday[8] + 0.7, "and it is a weekday shape");
        assert!(
            (tuesday[3] - 0.3).abs() < 0.1,
            "a quiet weekday hour stays near the baseline"
        );
        assert!(
            (saturday[8] - 0.3).abs() < 0.1,
            "no peak on a weekend: {}",
            saturday[8]
        );
    }

    #[test]
    fn learn_profile_ignores_samples_older_than_the_window() {
        let cfg = LearnConfig {
            rolling_window_days: 7,
            ..LearnConfig::default()
        };
        let mut samples = minutes(7, |_| 0.3);
        samples.extend(
            minutes(30, |_| 9.0)
                .into_iter()
                .filter(|s| s.ts < now() - Duration::days(8)),
        );
        let p = learn_profile(&samples, now(), &cfg, None).unwrap();
        assert!(p
            .daytime_profile_kw
            .iter()
            .flatten()
            .all(|v| (*v - 0.3).abs() < 0.05));
    }

    #[test]
    fn learn_profile_seasonal_factor_is_near_one_for_a_stationary_pattern() {
        let p = learn_profile(
            &minutes(28, weekday_coffee),
            now(),
            &LearnConfig::default(),
            None,
        )
        .unwrap();
        assert!(
            (p.seasonal_factor - 1.0).abs() < 0.05,
            "{}",
            p.seasonal_factor
        );
    }

    #[test]
    fn learn_profile_recent_error_is_none_without_a_previous_and_small_for_the_same_pattern() {
        let samples = minutes(28, weekday_coffee);
        let first = learn_profile(&samples, now(), &LearnConfig::default(), None).unwrap();
        assert_eq!(first.recent_mean_abs_error_kw, None);
        let second = learn_profile(&samples, now(), &LearnConfig::default(), Some(&first)).unwrap();
        assert!(second.recent_mean_abs_error_kw.unwrap() < 0.05);
    }

    #[test]
    fn learn_profile_recent_error_is_larger_after_a_step_change() {
        let first =
            learn_profile(&minutes(28, |_| 0.3), now(), &LearnConfig::default(), None).unwrap();
        let second = learn_profile(
            &minutes(7, |_| 1.3),
            now(),
            &LearnConfig::default(),
            Some(&first),
        )
        .unwrap();
        assert!(second.recent_mean_abs_error_kw.unwrap() > 0.9);
    }

    #[test]
    fn sample_profile_kw_reads_weekday_and_hour_and_scales_by_the_season() {
        let mut profile: [Vec<f64>; 7] = Default::default();
        profile[1] = (0..24).map(|h| h as f64).collect(); // Tuesday
        let tue_9 = Utc.with_ymd_and_hms(2026, 7, 14, 9, 30, 0).unwrap();
        assert_eq!(sample_profile_kw(&profile, 1.0, tue_9), 9.0);
        assert_eq!(sample_profile_kw(&profile, 1.5, tue_9), 13.5);
        let wed_9 = Utc.with_ymd_and_hms(2026, 7, 15, 9, 0, 0).unwrap();
        assert_eq!(
            sample_profile_kw(&profile, 1.0, wed_9),
            0.0,
            "an empty curve gives 0, never a panic"
        );
    }

    #[test]
    fn shrinkage_blend_is_zero_half_and_towards_one() {
        assert_eq!(shrinkage_blend(0.0, 2.5), 0.0);
        assert!((shrinkage_blend(2.5, 2.5) - 0.5).abs() < 1e-12);
        assert!(shrinkage_blend(1000.0, 2.5) > 0.99);
    }
}
