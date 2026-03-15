/// Asset history ring-buffer for time-series analytics.
///
/// Data structure only — not wired to live data in this speckit (speckit 002).
/// Callers will be added in a subsequent speckit.
use chrono::{DateTime, Utc};
use std::collections::{HashMap, VecDeque};

/// A single row in the timeline output.
#[derive(Debug, Clone)]
pub struct AssetTimelinePoint {
    pub ts: DateTime<Utc>,
    pub values: HashMap<String, f64>,
}

/// Rolling window of per-asset numeric state values.
///
/// Maintains a fixed-capacity ring buffer. Each `push()` appends a row and
/// evicts the oldest if at capacity. Columns are sparse: missing keys for a
/// given row are stored as `f64::NAN` to preserve alignment.
#[derive(Debug, Clone)]
pub struct AssetHistoryBuffer {
    timestamps: VecDeque<DateTime<Utc>>,
    columns: HashMap<String, VecDeque<f64>>,
    capacity: usize,
}

impl AssetHistoryBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            timestamps: VecDeque::with_capacity(capacity),
            columns: HashMap::new(),
            capacity,
        }
    }

    /// Append a row. Evicts the oldest row when at capacity.
    /// Columns not present in `values` receive `f64::NAN` for this row.
    /// New columns added by `values` are backfilled with NAN for all prior rows.
    pub fn push(&mut self, ts: DateTime<Utc>, values: HashMap<String, f64>) {
        let current_len = self.timestamps.len();

        // Evict oldest if at capacity.
        if current_len >= self.capacity {
            self.timestamps.pop_front();
            for col in self.columns.values_mut() {
                col.pop_front();
            }
        }

        let new_len = self.timestamps.len(); // after possible eviction

        // Backfill any new columns with NAN for all existing rows.
        for key in values.keys() {
            self.columns
                .entry(key.clone())
                .or_insert_with(|| VecDeque::from(vec![f64::NAN; new_len]));
        }

        self.timestamps.push_back(ts);

        // Append values; NAN for columns not in this row.
        for (key, col) in &mut self.columns {
            col.push_back(values.get(key).copied().unwrap_or(f64::NAN));
        }
    }

    /// Return row-oriented timeline points, optionally filtered to a time window.
    pub fn to_timeline(
        &self,
        window: Option<(DateTime<Utc>, DateTime<Utc>)>,
    ) -> Vec<AssetTimelinePoint> {
        self.timestamps
            .iter()
            .enumerate()
            .filter(|(_, ts)| {
                if let Some((start, end)) = window {
                    **ts >= start && **ts <= end
                } else {
                    true
                }
            })
            .map(|(i, ts)| {
                let values = self
                    .columns
                    .iter()
                    .map(|(k, col)| (k.clone(), col[i]))
                    .collect();
                AssetTimelinePoint { ts: *ts, values }
            })
            .collect()
    }

    pub fn len(&self) -> usize {
        self.timestamps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.timestamps.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn ts(offset_s: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000 + offset_s, 0).unwrap()
    }

    #[test]
    fn push_and_retrieve() {
        let mut buf = AssetHistoryBuffer::new(10);
        buf.push(ts(0), [("soc".into(), 0.5), ("power_kw".into(), 2.0)].into());
        buf.push(ts(1), [("soc".into(), 0.51)].into());

        assert_eq!(buf.len(), 2);
        let points = buf.to_timeline(None);
        assert_eq!(points.len(), 2);
        assert!((points[0].values["soc"] - 0.5).abs() < 1e-9);
        assert!(points[1].values["power_kw"].is_nan()); // missing → NAN
    }

    #[test]
    fn evicts_oldest_at_capacity() {
        let mut buf = AssetHistoryBuffer::new(3);
        for i in 0..5i64 {
            buf.push(ts(i), [("v".into(), i as f64)].into());
        }
        assert_eq!(buf.len(), 3);
        let points = buf.to_timeline(None);
        // Oldest three are ts(2), ts(3), ts(4)
        assert!((points[0].values["v"] - 2.0).abs() < 1e-9);
        assert!((points[2].values["v"] - 4.0).abs() < 1e-9);
    }

    #[test]
    fn window_filter() {
        let mut buf = AssetHistoryBuffer::new(10);
        for i in 0..5i64 {
            buf.push(ts(i), [("v".into(), i as f64)].into());
        }
        let points = buf.to_timeline(Some((ts(1), ts(3))));
        assert_eq!(points.len(), 3); // ts(1), ts(2), ts(3)
    }
}
