use chrono::{DateTime, Duration, Utc};
use std::collections::VecDeque;

use super::AssetState;

/// One recorded tick in a per-asset history buffer.
#[derive(Debug, Clone)]
pub struct HistoryPoint {
    pub ts: DateTime<Utc>,
    /// Signed: positive = import from grid, negative = export.
    pub power_kw: f64,
    /// Full state snapshot at this tick.
    pub state: AssetState,
}

/// Rolling per-asset ring buffer of `HistoryPoint` values.
///
/// Capacity defaults to 3600 entries ≈ 1 h at 1 s tick rate.
/// Oldest point is evicted automatically when full.
#[derive(Debug, Clone)]
pub struct AssetHistoryBuffer {
    capacity: usize,
    points: VecDeque<HistoryPoint>,
}

impl AssetHistoryBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            points: VecDeque::with_capacity(capacity),
        }
    }

    /// Append a point, evicting the oldest when at capacity.
    pub fn push(&mut self, point: HistoryPoint) {
        if self.points.len() == self.capacity {
            self.points.pop_front();
        }
        self.points.push_back(point);
    }

    /// All points in `[now − window, now]`, ordered ascending.
    pub fn slice(&self, window: Duration, now: DateTime<Utc>) -> Vec<HistoryPoint> {
        let start = now - window;
        self.points
            .iter()
            .filter(|p| p.ts >= start && p.ts <= now)
            .cloned()
            .collect()
    }

    /// Most recent point.
    pub fn latest(&self) -> Option<&HistoryPoint> {
        self.points.back()
    }

    /// Last-observation-carried-forward power at or before `t`.
    /// Returns `None` if no point exists at or before `t`.
    pub fn power_at(&self, t: DateTime<Utc>) -> Option<f64> {
        self.points
            .iter()
            .rev()
            .find(|p| p.ts <= t)
            .map(|p| p.power_kw)
    }

    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// Time-weighted average of `power_kw` over the last `window` ending at `now`.
    ///
    /// Uses LOCF (last-observation-carried-forward): each recorded value is held
    /// until the next point. Falls back to the single latest value when no points
    /// fall within the window.
    pub fn recent_avg_power(&self, window: Duration, now: DateTime<Utc>) -> Option<f64> {
        let window_start = now - window;
        let in_window: Vec<_> = self
            .points
            .iter()
            .filter(|p| p.ts >= window_start && p.ts <= now)
            .collect();

        if in_window.is_empty() {
            return self.latest().map(|p| p.power_kw);
        }

        let mut weighted_sum = 0.0f64;
        let mut total_weight = 0.0f64;

        // Seed the LOCF value: the last point *before* the window start, if any.
        let seed_power = self
            .points
            .iter()
            .rev()
            .find(|p| p.ts < window_start)
            .map(|p| p.power_kw)
            .unwrap_or(in_window[0].power_kw);

        let mut last_power = seed_power;
        let mut last_t_ms = window_start.timestamp_millis();

        for pt in &in_window {
            let t_ms = pt.ts.timestamp_millis();
            if t_ms > last_t_ms {
                let dt = (t_ms - last_t_ms) as f64;
                weighted_sum += last_power * dt;
                total_weight += dt;
            }
            last_power = pt.power_kw;
            last_t_ms = t_ms;
        }

        // Carry the last in-window point forward to `now`.
        let now_ms = now.timestamp_millis();
        if now_ms > last_t_ms {
            let dt = (now_ms - last_t_ms) as f64;
            weighted_sum += last_power * dt;
            total_weight += dt;
        }

        if total_weight > 0.0 {
            Some(weighted_sum / total_weight)
        } else {
            self.latest().map(|p| p.power_kw)
        }
    }
}

#[cfg(test)]
mod history_buffer_tests {
    use super::*;

    fn make_point(ts: DateTime<Utc>, power_kw: f64) -> HistoryPoint {
        // Use a battery state as a stand-in — the type is irrelevant for power tests.
        HistoryPoint {
            ts,
            power_kw,
            state: AssetState::Battery(crate::assets::battery::BatteryState {
                soc: 0.5,
                actual_power_kw: power_kw,
            }),
        }
    }

    fn secs(n: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(n, 0).unwrap()
    }

    // ── recent_avg_power ─────────────────────────────────────────────────────

    #[test]
    fn recent_avg_power_empty_buffer_returns_none() {
        let buf = AssetHistoryBuffer::new(100);
        let now = secs(100);
        assert!(buf.recent_avg_power(Duration::seconds(60), now).is_none());
    }

    #[test]
    fn recent_avg_power_constant_power_returns_that_power() {
        let mut buf = AssetHistoryBuffer::new(100);
        let now = secs(100);
        for i in 0..10 {
            buf.push(make_point(secs(i * 10), 1.5));
        }
        let avg = buf.recent_avg_power(Duration::seconds(60), now).unwrap();
        assert!((avg - 1.5).abs() < 1e-9, "expected 1.5, got {avg}");
    }

    #[test]
    fn recent_avg_power_alternating_returns_time_weighted_mean() {
        // 10 points alternating 0 and 2.5 at 1-second intervals in [0, 9].
        // Window = [0, 10]. Average should be ≈ 1.25 kW.
        let mut buf = AssetHistoryBuffer::new(100);
        for i in 0..10i64 {
            let power = if i % 2 == 0 { 0.0 } else { 2.5 };
            buf.push(make_point(secs(i), power));
        }
        let now = secs(10);
        let avg = buf.recent_avg_power(Duration::seconds(10), now).unwrap();
        // Tolerance of 0.5 kW: LOCF means off periods contribute 0, on periods 2.5
        assert!(avg > 0.5 && avg < 2.0, "expected ~1.25, got {avg}");
    }

    #[test]
    fn recent_avg_power_all_points_before_window_returns_latest() {
        // All points are older than the window → fallback to latest().
        let mut buf = AssetHistoryBuffer::new(100);
        buf.push(make_point(secs(0), 3.0));
        buf.push(make_point(secs(1), 4.0));
        let now = secs(100); // window = [40, 100], all points at t<40
        let avg = buf.recent_avg_power(Duration::seconds(60), now).unwrap();
        assert!((avg - 4.0).abs() < 1e-9, "expected 4.0 (latest), got {avg}");
    }

    #[test]
    fn recent_avg_power_window_larger_than_buffer_uses_all_points() {
        let mut buf = AssetHistoryBuffer::new(100);
        for i in 0..5 {
            buf.push(make_point(secs(i), 2.0));
        }
        let now = secs(4);
        let avg = buf.recent_avg_power(Duration::hours(1), now).unwrap();
        assert!((avg - 2.0).abs() < 1e-9, "expected 2.0, got {avg}");
    }
}
