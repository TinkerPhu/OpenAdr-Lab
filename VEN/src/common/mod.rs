use chrono::{DateTime, Duration, Utc};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interpolation {
    Linear,
    Step,
}

#[derive(Debug, Clone)]
pub enum Aggregation {
    Mean,
    Min,
    Max,
}

#[derive(Debug, Clone)]
pub struct TimeSeries {
    pub samples: Vec<(DateTime<Utc>, f64)>,
    pub interpolation: Interpolation,
}

impl TimeSeries {
    pub fn empty(interpolation: Interpolation) -> Self {
        Self {
            samples: Vec::new(),
            interpolation,
        }
    }

    /// Verify that all timestamps are strictly ascending.
    pub fn is_ascending(&self) -> bool {
        self.samples.windows(2).all(|w| w[0].0 < w[1].0)
    }

    /// Evaluate the series at an arbitrary timestamp using the declared interpolation mode.
    ///
    /// - Before first sample → `None`
    /// - After last sample: Step → LOCF, Linear → `None`
    /// - Between samples: Step → left value (LOCF), Linear → proportional
    pub fn interpolate_at(&self, ts: DateTime<Utc>) -> Option<f64> {
        if self.samples.is_empty() {
            return None;
        }

        // Binary search: find first sample with timestamp > ts
        let pos = self.samples.partition_point(|(t, _)| *t <= ts);

        if pos == 0 {
            // ts is before the first sample
            return None;
        }

        let (t_left, v_left) = self.samples[pos - 1];
        if t_left == ts {
            // Exact match
            return Some(v_left);
        }

        // ts is after the last sample
        if pos == self.samples.len() {
            return match self.interpolation {
                Interpolation::Step => Some(v_left), // LOCF
                Interpolation::Linear => None,       // no extrapolation
            };
        }

        // Between two samples
        match self.interpolation {
            Interpolation::Step => Some(v_left),
            Interpolation::Linear => {
                let (t_right, v_right) = self.samples[pos];
                let frac = (ts - t_left).num_milliseconds() as f64
                    / (t_right - t_left).num_milliseconds() as f64;
                Some(v_left + (v_right - v_left) * frac)
            }
        }
    }

    /// Compute the time-weighted mean of the signal over `[start, end)`.
    ///
    /// Returns `None` if the signal is undefined at `start` (i.e. before data).
    /// Used by the MILP input builder so a plan slot straddling a tariff
    /// boundary is priced at the true blended rate, not the slot-start rate.
    pub fn time_weighted_mean(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Option<f64> {
        if start >= end || self.samples.is_empty() {
            return None;
        }

        let v_start = self.interpolate_at(start)?;

        // Build ordered split points: start, interior sample timestamps, end.
        let mut splits: Vec<DateTime<Utc>> = vec![start];
        for &(t, _) in &self.samples {
            if t > start && t < end {
                splits.push(t);
            }
        }
        splits.push(end);

        let total_dur = (end - start).num_milliseconds() as f64;
        let mut weighted_sum = 0.0;

        for w in splits.windows(2) {
            let seg_start = w[0];
            let seg_end = w[1];
            let dur = (seg_end - seg_start).num_milliseconds() as f64;
            if dur <= 0.0 {
                continue;
            }

            // We know interpolate_at(start) succeeded, so all points within
            // [start, end) are defined (Step carries forward, Linear interpolates
            // between known samples).
            let va = if seg_start == start {
                v_start
            } else {
                self.interpolate_at(seg_start).unwrap_or(v_start)
            };

            match self.interpolation {
                Interpolation::Step => {
                    // Step/LOCF: value is constant throughout each sub-interval.
                    // Always defined after the first sample (LOCF carries forward).
                    weighted_sum += va * dur;
                }
                Interpolation::Linear => {
                    // Linear: need both endpoints. If the signal is undefined at
                    // seg_end (past last sample), the bucket is not fully covered.
                    let vb = self.interpolate_at(seg_end)?;
                    weighted_sum += (va + vb) / 2.0 * dur;
                }
            }
        }

        Some(weighted_sum / total_dur)
    }

    /// Compute the minimum value of the signal over `[start, end)`.
    ///
    /// For Step: checks LOCF values at each change-point within the bucket.
    /// For Linear: each sub-segment is monotonic, so extremes occur at endpoints.
    fn bucket_min(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Option<f64> {
        self.bucket_extreme(start, end, |a, b| a.min(b))
    }

    /// Compute the maximum value of the signal over `[start, end)`.
    fn bucket_max(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Option<f64> {
        self.bucket_extreme(start, end, |a, b| a.max(b))
    }

    /// Shared logic for bucket_min / bucket_max.
    fn bucket_extreme(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        pick: impl Fn(f64, f64) -> f64,
    ) -> Option<f64> {
        if start >= end || self.samples.is_empty() {
            return None;
        }

        let v_start = self.interpolate_at(start)?;
        let mut result = v_start;

        // Check all interior sample points
        for &(t, _) in &self.samples {
            if t > start && t < end {
                if let Some(v) = self.interpolate_at(t) {
                    result = pick(result, v);
                }
            }
        }

        // For Linear, also check the end boundary (value may differ)
        if matches!(self.interpolation, Interpolation::Linear) {
            if let Some(v_end) = self.interpolate_at(end) {
                result = pick(result, v_end);
            }
        }

        Some(result)
    }

    /// Resample onto an arbitrary timestamp grid. Each output point is the
    /// interpolated value at that timestamp. Points where interpolation is
    /// undefined are skipped.
    pub fn resample_to_grid(&self, timestamps: &[DateTime<Utc>]) -> TimeSeries {
        let samples: Vec<(DateTime<Utc>, f64)> = timestamps
            .iter()
            .filter_map(|&ts| self.interpolate_at(ts).map(|v| (ts, v)))
            .collect();

        TimeSeries {
            samples,
            interpolation: self.interpolation.clone(),
        }
    }

    /// Resample onto a regular grid with bucket-width `width`.
    ///
    /// Aggregation mode controls how values within each bucket are combined:
    /// - `Mean`: time-weighted mean (correct for tariffs/prices)
    /// - `Min`: minimum value in bucket (correct for capacity limits)
    /// - `Max`: maximum value in bucket
    ///
    /// Grid timestamps are aligned to epoch-based boundaries:
    /// - First bucket starts at `ceil(first_sample, width)`
    /// - Last bucket starts at or before `floor(last_sample, width)`
    ///
    /// This ensures series from different assets share timestamps after resampling.
    pub fn resample_uniform(&self, width: Duration, agg: Aggregation) -> TimeSeries {
        let width_ms = width.num_milliseconds();
        if self.samples.is_empty() || width_ms <= 0 {
            return TimeSeries {
                samples: Vec::new(),
                interpolation: self.interpolation.clone(),
            };
        }

        let first_ts = self.samples.first().unwrap().0;
        let last_ts = self.samples.last().unwrap().0;

        let grid_start = ceil_to_grid(first_ts, width_ms);
        let grid_end = floor_to_grid(last_ts, width_ms);

        let mut samples = Vec::new();
        let mut t = grid_start;
        while t <= grid_end {
            let bucket_end = t + width;
            let value = match agg {
                Aggregation::Mean => self.time_weighted_mean(t, bucket_end),
                Aggregation::Min => self.bucket_min(t, bucket_end),
                Aggregation::Max => self.bucket_max(t, bucket_end),
            };
            if let Some(v) = value {
                samples.push((t, v));
            }
            t += width;
        }

        TimeSeries {
            samples,
            interpolation: self.interpolation.clone(),
        }
    }
}

/// Round timestamp down to the nearest grid boundary.
fn floor_to_grid(ts: DateTime<Utc>, width_ms: i64) -> DateTime<Utc> {
    let ts_ms = ts.timestamp_millis();
    let floored = ts_ms - ts_ms.rem_euclid(width_ms);
    DateTime::from_timestamp_millis(floored).unwrap()
}

/// Round timestamp up to the nearest grid boundary.
fn ceil_to_grid(ts: DateTime<Utc>, width_ms: i64) -> DateTime<Utc> {
    let ts_ms = ts.timestamp_millis();
    let rem = ts_ms.rem_euclid(width_ms);
    if rem == 0 {
        ts
    } else {
        DateTime::from_timestamp_millis(ts_ms + (width_ms - rem)).unwrap()
    }
}

// ---------------------------------------------------------------------------
// ISO 8601 duration parser
// ---------------------------------------------------------------------------

/// Parse an ISO 8601 duration string into total seconds.
///
/// Supports: `PT1H`, `PT15M`, `PT30S`, `PT1H30M`, `P1D`, `P1M`, `P1Y`,
/// and combinations thereof (e.g. `P1DT6H`).
/// Year and month are approximated: 1Y = 365 days, 1M = 30 days.
/// Returns 3600 (1 hour) as a fallback for unparseable or zero-result strings.
pub(crate) fn parse_iso8601_duration_secs(s: &str) -> i64 {
    let s = s.trim();
    if !s.starts_with('P') {
        return 3600; // fallback: 1 hour
    }
    let rest = &s[1..];
    let (date_part, time_part) = if let Some(t_pos) = rest.find('T') {
        (&rest[..t_pos], &rest[t_pos + 1..])
    } else {
        (rest, "")
    };

    let mut total_secs: i64 = 0;

    let mut buf = String::new();
    for ch in date_part.chars() {
        if ch.is_ascii_digit() {
            buf.push(ch);
        } else if ch == 'Y' {
            let v: i64 = buf.parse().unwrap_or(0);
            total_secs += v * 365 * 86400;
            buf.clear();
        } else if ch == 'M' {
            let v: i64 = buf.parse().unwrap_or(0);
            total_secs += v * 30 * 86400;
            buf.clear();
        } else if ch == 'D' {
            let v: i64 = buf.parse().unwrap_or(0);
            total_secs += v * 86400;
            buf.clear();
        } else {
            buf.clear();
        }
    }

    buf.clear();
    for ch in time_part.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            buf.push(ch);
        } else if ch == 'H' {
            let v: f64 = buf.parse().unwrap_or(0.0);
            total_secs += v as i64 * 3600;
            buf.clear();
        } else if ch == 'M' {
            let v: f64 = buf.parse().unwrap_or(0.0);
            total_secs += v as i64 * 60;
            buf.clear();
        } else if ch == 'S' {
            let v: f64 = buf.parse().unwrap_or(0.0);
            total_secs += v as i64;
            buf.clear();
        } else {
            buf.clear();
        }
    }

    if total_secs <= 0 {
        3600
    } else {
        total_secs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};

    fn mean() -> Aggregation {
        Aggregation::Mean
    }

    /// Helper: build a fixed base time for deterministic tests.
    fn t(hour: u32, min: u32, sec: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 3, 21, hour, min, sec).unwrap()
    }

    fn step_series(samples: Vec<(DateTime<Utc>, f64)>) -> TimeSeries {
        TimeSeries {
            samples,
            interpolation: Interpolation::Step,
        }
    }

    fn linear_series(samples: Vec<(DateTime<Utc>, f64)>) -> TimeSeries {
        TimeSeries {
            samples,
            interpolation: Interpolation::Linear,
        }
    }

    // ── existing tests ──────────────────────────────────────────────

    #[test]
    fn empty_series_has_no_samples() {
        let s = TimeSeries::empty(Interpolation::Linear);
        assert!(s.samples.is_empty());
    }

    #[test]
    fn empty_series_is_trivially_ascending() {
        let s = TimeSeries::empty(Interpolation::Linear);
        assert!(s.is_ascending());
    }

    #[test]
    fn non_empty_series_ascending_check() {
        let now = Utc::now();
        let s = TimeSeries {
            samples: vec![(now, 1.0), (now + Duration::seconds(60), 2.0)],
            interpolation: Interpolation::Linear,
        };
        assert!(s.is_ascending());
    }

    #[test]
    fn series_with_same_timestamp_not_ascending() {
        let now = Utc::now();
        let s = TimeSeries {
            samples: vec![(now, 1.0), (now, 2.0)],
            interpolation: Interpolation::Linear,
        };
        assert!(!s.is_ascending());
    }

    // ── interpolate_at ──────────────────────────────────────────────

    #[test]
    fn interpolate_at_empty_series() {
        let s = linear_series(vec![]);
        assert!(s.interpolate_at(t(12, 0, 0)).is_none());
    }

    #[test]
    fn interpolate_at_single_point_exact() {
        let s = linear_series(vec![(t(12, 0, 0), 5.0)]);
        assert_eq!(s.interpolate_at(t(12, 0, 0)), Some(5.0));
    }

    #[test]
    fn interpolate_at_single_point_before() {
        let s = linear_series(vec![(t(12, 0, 0), 5.0)]);
        assert!(s.interpolate_at(t(11, 0, 0)).is_none());
    }

    #[test]
    fn interpolate_at_single_point_after_step() {
        let s = step_series(vec![(t(12, 0, 0), 5.0)]);
        assert_eq!(s.interpolate_at(t(13, 0, 0)), Some(5.0)); // LOCF
    }

    #[test]
    fn interpolate_at_single_point_after_linear() {
        let s = linear_series(vec![(t(12, 0, 0), 5.0)]);
        assert!(s.interpolate_at(t(13, 0, 0)).is_none()); // no extrapolation
    }

    #[test]
    fn interpolate_at_between_step() {
        let s = step_series(vec![(t(10, 0, 0), 3.0), (t(11, 0, 0), 7.0)]);
        assert_eq!(s.interpolate_at(t(10, 30, 0)), Some(3.0)); // LOCF
    }

    #[test]
    fn interpolate_at_between_linear() {
        let s = linear_series(vec![(t(10, 0, 0), 0.0), (t(11, 0, 0), 10.0)]);
        let v = s.interpolate_at(t(10, 30, 0)).unwrap();
        assert!((v - 5.0).abs() < 1e-9); // halfway → 5.0
    }

    #[test]
    fn interpolate_at_exact_middle_point() {
        let s = linear_series(vec![
            (t(10, 0, 0), 1.0),
            (t(11, 0, 0), 2.0),
            (t(12, 0, 0), 3.0),
        ]);
        assert_eq!(s.interpolate_at(t(11, 0, 0)), Some(2.0));
    }

    #[test]
    fn interpolate_at_before_first_of_multi() {
        let s = step_series(vec![(t(10, 0, 0), 1.0), (t(11, 0, 0), 2.0)]);
        assert!(s.interpolate_at(t(9, 0, 0)).is_none());
    }

    // ── time_weighted_mean ──────────────────────────────────────────

    #[test]
    fn twm_constant_step_signal() {
        // Constant value 5.0 over entire bucket
        let s = step_series(vec![(t(10, 0, 0), 5.0)]);
        let mean = s.time_weighted_mean(t(10, 0, 0), t(10, 10, 0)).unwrap();
        assert!((mean - 5.0).abs() < 1e-9);
    }

    #[test]
    fn twm_step_with_boundary() {
        // 5.0 from 10:00, changes to 15.0 at 10:10. Bucket [10:05, 10:15).
        // TWM = (5min×5.0 + 5min×15.0) / 10min = 10.0
        let s = step_series(vec![(t(10, 0, 0), 5.0), (t(10, 10, 0), 15.0)]);
        let mean = s.time_weighted_mean(t(10, 5, 0), t(10, 15, 0)).unwrap();
        assert!((mean - 10.0).abs() < 1e-9);
    }

    #[test]
    fn twm_linear_ramp() {
        // Linear ramp from 0.0 at 10:00 to 10.0 at 10:10.
        // TWM over [10:00, 10:10) = (0 + 10) / 2 = 5.0 (trapezoid)
        let s = linear_series(vec![(t(10, 0, 0), 0.0), (t(10, 10, 0), 10.0)]);
        let mean = s.time_weighted_mean(t(10, 0, 0), t(10, 10, 0)).unwrap();
        assert!((mean - 5.0).abs() < 1e-9);
    }

    #[test]
    fn twm_bucket_before_data() {
        let s = step_series(vec![(t(12, 0, 0), 5.0)]);
        assert!(s.time_weighted_mean(t(11, 0, 0), t(11, 30, 0)).is_none());
    }

    #[test]
    fn twm_start_ge_end() {
        let s = step_series(vec![(t(10, 0, 0), 5.0)]);
        assert!(s.time_weighted_mean(t(10, 5, 0), t(10, 5, 0)).is_none());
        assert!(s.time_weighted_mean(t(10, 10, 0), t(10, 5, 0)).is_none());
    }

    #[test]
    fn twm_step_three_segments() {
        // 1.0 from 10:00, 2.0 from 10:10, 3.0 from 10:20
        // Bucket [10:05, 10:25):
        //   5min×1.0 + 10min×2.0 + 5min×3.0 = 5+20+15 = 40 / 20min = 2.0
        let s = step_series(vec![
            (t(10, 0, 0), 1.0),
            (t(10, 10, 0), 2.0),
            (t(10, 20, 0), 3.0),
        ]);
        let mean = s.time_weighted_mean(t(10, 5, 0), t(10, 25, 0)).unwrap();
        assert!((mean - 2.0).abs() < 1e-9);
    }

    // ── resample_to_grid ────────────────────────────────────────────

    #[test]
    fn resample_to_grid_within_data() {
        let s = linear_series(vec![(t(10, 0, 0), 0.0), (t(11, 0, 0), 60.0)]);
        let grid = vec![t(10, 15, 0), t(10, 30, 0), t(10, 45, 0)];
        let r = s.resample_to_grid(&grid);
        assert_eq!(r.samples.len(), 3);
        assert!((r.samples[0].1 - 15.0).abs() < 1e-9);
        assert!((r.samples[1].1 - 30.0).abs() < 1e-9);
        assert!((r.samples[2].1 - 45.0).abs() < 1e-9);
    }

    #[test]
    fn resample_to_grid_outside_data_skipped() {
        let s = linear_series(vec![(t(10, 0, 0), 0.0), (t(11, 0, 0), 60.0)]);
        let grid = vec![t(9, 0, 0), t(10, 30, 0), t(12, 0, 0)];
        let r = s.resample_to_grid(&grid);
        // Before first and after last (Linear) are skipped
        assert_eq!(r.samples.len(), 1);
        assert!((r.samples[0].1 - 30.0).abs() < 1e-9);
    }

    #[test]
    fn resample_to_grid_step_after_last_included() {
        let s = step_series(vec![(t(10, 0, 0), 5.0), (t(11, 0, 0), 10.0)]);
        let grid = vec![t(9, 0, 0), t(10, 30, 0), t(12, 0, 0)];
        let r = s.resample_to_grid(&grid);
        // Before first skipped; between → LOCF 5.0; after last → LOCF 10.0
        assert_eq!(r.samples.len(), 2);
        assert_eq!(r.samples[0].1, 5.0);
        assert_eq!(r.samples[1].1, 10.0);
    }

    #[test]
    fn resample_to_grid_empty_input() {
        let s = linear_series(vec![(t(10, 0, 0), 1.0)]);
        let r = s.resample_to_grid(&[]);
        assert!(r.samples.is_empty());
    }

    #[test]
    fn resample_to_grid_step_vs_linear_differ() {
        let step = step_series(vec![(t(10, 0, 0), 0.0), (t(11, 0, 0), 10.0)]);
        let lin = linear_series(vec![(t(10, 0, 0), 0.0), (t(11, 0, 0), 10.0)]);
        let grid = vec![t(10, 30, 0)];

        let r_step = step.resample_to_grid(&grid);
        let r_lin = lin.resample_to_grid(&grid);

        assert_eq!(r_step.samples[0].1, 0.0); // LOCF
        assert!((r_lin.samples[0].1 - 5.0).abs() < 1e-9); // Linear midpoint
    }

    // ── resample_uniform ────────────────────────────────────────────

    #[test]
    fn resample_uniform_empty() {
        let s = step_series(vec![]);
        let r = s.resample_uniform(Duration::minutes(5), mean());
        assert!(r.samples.is_empty());
    }

    #[test]
    fn resample_uniform_tariff_boundary() {
        // Tariff: 0.20 from 10:00, changes to 0.15 at 11:00 (step)
        let s = step_series(vec![(t(10, 0, 0), 0.20), (t(11, 0, 0), 0.15)]);
        let r = s.resample_uniform(Duration::minutes(5), mean());

        // Grid: 10:00, 10:05, 10:10, ..., 10:55, 11:00
        // Bucket [10:55, 11:00): entirely 0.20
        let bucket_1055 = r.samples.iter().find(|(ts, _)| *ts == t(10, 55, 0));
        assert!(bucket_1055.is_some());
        assert!((bucket_1055.unwrap().1 - 0.20).abs() < 1e-9);

        // Bucket [11:00, 11:05): entirely 0.15 (LOCF from 11:00)
        let bucket_1100 = r.samples.iter().find(|(ts, _)| *ts == t(11, 0, 0));
        assert!(bucket_1100.is_some());
        assert!((bucket_1100.unwrap().1 - 0.15).abs() < 1e-9);
    }

    #[test]
    fn resample_uniform_forecast_alignment() {
        // Data starts at 12:22 — first bucket should be at 12:25 (ceil)
        let s = step_series(vec![(t(12, 22, 0), 1.0), (t(12, 40, 0), 2.0)]);
        let r = s.resample_uniform(Duration::minutes(5), mean());
        assert!(!r.samples.is_empty());
        assert_eq!(r.samples[0].0, t(12, 25, 0));
    }

    #[test]
    fn resample_uniform_history_alignment() {
        // Data ends at 12:22 — last bucket starts at 12:20 (floor)
        let s = step_series(vec![(t(12, 0, 0), 1.0), (t(12, 22, 0), 2.0)]);
        let r = s.resample_uniform(Duration::minutes(5), mean());
        let last = r.samples.last().unwrap();
        assert_eq!(last.0, t(12, 20, 0));
    }

    #[test]
    fn resample_uniform_linear_ramp() {
        // Linear ramp: 0.0 at 10:00, 60.0 at 11:00
        // 5-min buckets. Bucket [10:00, 10:05): TWM = trapezoid(0, 5) = 2.5
        let s = linear_series(vec![(t(10, 0, 0), 0.0), (t(11, 0, 0), 60.0)]);
        let r = s.resample_uniform(Duration::minutes(5), mean());
        assert_eq!(r.samples.len(), 12); // 10:00..10:55
                                         // First bucket [10:00, 10:05): mean of linear from 0→5 = 2.5
        assert!((r.samples[0].1 - 2.5).abs() < 1e-9);
        // Last bucket [10:55, 11:00): mean of linear from 55→60 = 57.5
        assert!((r.samples[11].1 - 57.5).abs() < 1e-9);
    }

    #[test]
    fn resample_uniform_width_larger_than_range() {
        let s = step_series(vec![(t(10, 0, 0), 5.0), (t(10, 3, 0), 7.0)]);
        // 5-min width, data spans 3 minutes → floor(10:03) = 10:00, ceil(10:00) = 10:00
        // But bucket [10:00, 10:05) extends past last sample — Step LOCF handles this
        let r = s.resample_uniform(Duration::minutes(5), mean());
        assert_eq!(r.samples.len(), 1);
        // TWM: 3min×5.0 + 2min×7.0 = 29 / 5 = 5.8
        assert!((r.samples[0].1 - 5.8).abs() < 1e-9);
    }

    #[test]
    fn resample_uniform_identity_step() {
        // Data already on a 5-min grid (Step). LOCF carries the last value
        // forward, so bucket [10:10, 10:15) is valid → 3 output buckets.
        let s = step_series(vec![
            (t(10, 0, 0), 1.0),
            (t(10, 5, 0), 2.0),
            (t(10, 10, 0), 3.0),
        ]);
        let r = s.resample_uniform(Duration::minutes(5), mean());
        assert_eq!(r.samples.len(), 3);
        assert!((r.samples[0].1 - 1.0).abs() < 1e-9);
        assert!((r.samples[1].1 - 2.0).abs() < 1e-9);
        assert!((r.samples[2].1 - 3.0).abs() < 1e-9);
    }

    #[test]
    fn resample_uniform_identity_linear() {
        // Data already on a 5-min grid (Linear). The last bucket [10:10, 10:15)
        // extends past the data → excluded. Only 2 output buckets.
        let s = linear_series(vec![
            (t(10, 0, 0), 1.0),
            (t(10, 5, 0), 2.0),
            (t(10, 10, 0), 3.0),
        ]);
        let r = s.resample_uniform(Duration::minutes(5), mean());
        assert_eq!(r.samples.len(), 2);
        assert!((r.samples[0].1 - 1.5).abs() < 1e-9); // TWM of linear 1→2
        assert!((r.samples[1].1 - 2.5).abs() < 1e-9); // TWM of linear 2→3
    }

    // ── resample_uniform with Min/Max ────────────────────────────────

    #[test]
    fn resample_uniform_min_step_mid_bucket_change() {
        // Step: 10.0 at 10:00, drops to 3.0 at 10:03
        // Bucket [10:00, 10:05): min = 3.0
        let s = step_series(vec![(t(10, 0, 0), 10.0), (t(10, 3, 0), 3.0)]);
        let r = s.resample_uniform(Duration::minutes(5), Aggregation::Min);
        assert_eq!(r.samples.len(), 1);
        assert!((r.samples[0].1 - 3.0).abs() < 1e-9);
    }

    #[test]
    fn resample_uniform_max_step_mid_bucket_change() {
        // Step: 3.0 at 10:00, jumps to 10.0 at 10:03
        // Bucket [10:00, 10:05): max = 10.0
        let s = step_series(vec![(t(10, 0, 0), 3.0), (t(10, 3, 0), 10.0)]);
        let r = s.resample_uniform(Duration::minutes(5), Aggregation::Max);
        assert_eq!(r.samples.len(), 1);
        assert!((r.samples[0].1 - 10.0).abs() < 1e-9);
    }

    #[test]
    fn resample_uniform_min_linear_ramp() {
        // Linear ramp 0→60 over 10:00–11:00
        // Bucket [10:00, 10:05): min at start = 0.0, max at end = 5.0
        let s = linear_series(vec![(t(10, 0, 0), 0.0), (t(11, 0, 0), 60.0)]);
        let r_min = s.resample_uniform(Duration::minutes(5), Aggregation::Min);
        let r_max = s.resample_uniform(Duration::minutes(5), Aggregation::Max);
        // First bucket: min=0.0, max=5.0
        assert!((r_min.samples[0].1 - 0.0).abs() < 1e-9);
        assert!((r_max.samples[0].1 - 5.0).abs() < 1e-9);
        // Last bucket [10:55, 11:00): min=55.0, max=60.0
        assert!((r_min.samples[11].1 - 55.0).abs() < 1e-9);
        assert!((r_max.samples[11].1 - 60.0).abs() < 1e-9);
    }

    #[test]
    fn resample_uniform_constant_min_max_equal_mean() {
        // Constant 5.0 → min = max = mean = 5.0
        let s = step_series(vec![(t(10, 0, 0), 5.0)]);
        let r_mean = s.resample_uniform(Duration::minutes(5), mean());
        let r_min = s.resample_uniform(Duration::minutes(5), Aggregation::Min);
        let r_max = s.resample_uniform(Duration::minutes(5), Aggregation::Max);
        assert_eq!(r_mean.samples.len(), 1);
        assert_eq!(r_min.samples.len(), 1);
        assert_eq!(r_max.samples.len(), 1);
        assert!((r_mean.samples[0].1 - 5.0).abs() < 1e-9);
        assert!((r_min.samples[0].1 - 5.0).abs() < 1e-9);
        assert!((r_max.samples[0].1 - 5.0).abs() < 1e-9);
    }

    #[test]
    fn resample_uniform_min_capacity_limit_across_boundary() {
        // Capacity limit: 10kW from 10:00, drops to 5kW at 10:57
        // 5-min bucket [10:55, 11:00): contains the drop at 10:57
        // Min should be 5.0 (the strictest limit)
        let s = step_series(vec![(t(10, 0, 0), 10.0), (t(10, 57, 0), 5.0)]);
        let r = s.resample_uniform(Duration::minutes(5), Aggregation::Min);
        let bucket_1055 = r.samples.iter().find(|(ts, _)| *ts == t(10, 55, 0));
        assert!(bucket_1055.is_some());
        assert!((bucket_1055.unwrap().1 - 5.0).abs() < 1e-9);
        // Earlier buckets should all be 10.0
        let bucket_1050 = r.samples.iter().find(|(ts, _)| *ts == t(10, 50, 0));
        assert!(bucket_1050.is_some());
        assert!((bucket_1050.unwrap().1 - 10.0).abs() < 1e-9);
    }

    // ── grid alignment helpers ──────────────────────────────────────

    #[test]
    fn floor_to_grid_exact() {
        let ts = t(10, 0, 0);
        assert_eq!(floor_to_grid(ts, 300_000), ts); // 5 min
    }

    #[test]
    fn floor_to_grid_rounds_down() {
        assert_eq!(floor_to_grid(t(10, 3, 0), 300_000), t(10, 0, 0));
        assert_eq!(floor_to_grid(t(10, 7, 0), 300_000), t(10, 5, 0));
    }

    #[test]
    fn ceil_to_grid_exact() {
        let ts = t(10, 0, 0);
        assert_eq!(ceil_to_grid(ts, 300_000), ts);
    }

    #[test]
    fn ceil_to_grid_rounds_up() {
        assert_eq!(ceil_to_grid(t(10, 1, 0), 300_000), t(10, 5, 0));
        assert_eq!(ceil_to_grid(t(10, 22, 0), 300_000), t(10, 25, 0));
    }

    // ── parse_iso8601_duration_secs ──────────────────────────────────

    #[test]
    fn test_parse_iso8601_duration_hour() {
        assert_eq!(parse_iso8601_duration_secs("PT1H"), 3600);
    }

    #[test]
    fn test_parse_iso8601_duration_minutes() {
        assert_eq!(parse_iso8601_duration_secs("PT15M"), 900);
        assert_eq!(parse_iso8601_duration_secs("PT5M"), 300);
    }

    #[test]
    fn test_parse_iso8601_duration_combined() {
        assert_eq!(parse_iso8601_duration_secs("PT1H30M"), 5400);
    }

    #[test]
    fn test_parse_iso8601_duration_days() {
        assert_eq!(parse_iso8601_duration_secs("P1D"), 86400);
    }

    #[test]
    fn test_parse_iso8601_duration_years() {
        let secs = parse_iso8601_duration_secs("P9999Y");
        assert!(
            secs > 9998i64 * 365 * 86400,
            "P9999Y should be a very large value"
        );
    }

    #[test]
    fn test_parse_iso8601_duration_months() {
        assert_eq!(parse_iso8601_duration_secs("P1M"), 30 * 86400);
    }

    #[test]
    fn test_parse_iso8601_duration_secs_day() {
        // Explicit coverage for the gap in the old reporter parser (which ignored date parts).
        assert_eq!(parse_iso8601_duration_secs("P1D"), 86400);
    }
}
