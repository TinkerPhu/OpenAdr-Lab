/// Controller observability: ControllerEvent log + asset history buffers.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use uuid::Uuid;

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

// ---------------------------------------------------------------------------
// ControllerEvent — tagged enum for significant controller decisions
// ---------------------------------------------------------------------------

/// A significant controller decision or state change, stored in the event log.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ControllerEvent {
    OpenAdrArrived {
        ts: DateTime<Utc>,
        event_name: String,
        signal_type: String,
        value: f64,
        interval: u32,
    },
    OpenAdrExpired {
        ts: DateTime<Utc>,
        event_name: String,
    },
    RateChange {
        ts: DateTime<Utc>,
        interval_start: DateTime<Utc>,
        import_eur_kwh: f64,
        export_eur_kwh: f64,
    },
    CapacityChange {
        ts: DateTime<Utc>,
        import_limit_kw: Option<f64>,
        export_limit_kw: Option<f64>,
    },
    PlanCycle {
        ts: DateTime<Utc>,
        trigger_reason: String,
        firm_slots: usize,
        flexible_slots: usize,
    },
    PacketTransition {
        ts: DateTime<Utc>,
        packet_id: Uuid,
        asset_id: String,
        from_status: String,
        to_status: String,
    },
    RequestTransition {
        ts: DateTime<Utc>,
        request_id: Uuid,
        asset_id: String,
        from_status: String,
        to_status: String,
    },
}

// ---------------------------------------------------------------------------
// ControllerEventLog — ring buffer of ControllerEvent
// ---------------------------------------------------------------------------

/// Ring buffer of `ControllerEvent` entries.
#[derive(Debug, Clone, Default)]
pub struct ControllerEventLog {
    entries: VecDeque<ControllerEvent>,
    capacity: usize,
}

impl ControllerEventLog {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub fn push(&mut self, event: ControllerEvent) {
        if self.entries.len() >= self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(event);
    }

    pub fn entries(&self) -> Vec<ControllerEvent> {
        self.entries.iter().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// ControllerTrace — combined holder for both observability buffers
// ---------------------------------------------------------------------------

/// Combined observability state: event log + per-asset history buffers.
#[derive(Debug, Clone)]
pub struct ControllerTrace {
    pub event_log: ControllerEventLog,
    pub asset_history: HashMap<String, AssetHistoryBuffer>,
}

impl ControllerTrace {
    pub fn new() -> Self {
        Self {
            event_log: ControllerEventLog::new(500),
            asset_history: HashMap::new(),
        }
    }

    pub fn push_event(&mut self, event: ControllerEvent) {
        self.event_log.push(event);
    }

    pub fn push_asset_row(
        &mut self,
        asset_id: &str,
        ts: DateTime<Utc>,
        values: HashMap<String, f64>,
    ) {
        self.asset_history
            .entry(asset_id.to_string())
            .or_insert_with(|| AssetHistoryBuffer::new(3600))
            .push(ts, values);
    }

    pub fn events(&self) -> Vec<ControllerEvent> {
        self.event_log.entries()
    }

    pub fn asset_history_for(&self, asset_id: &str) -> Option<&AssetHistoryBuffer> {
        self.asset_history.get(asset_id)
    }
}

impl Default for ControllerTrace {
    fn default() -> Self {
        Self::new()
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
        buf.push(
            ts(0),
            [("soc".into(), 0.5), ("power_kw".into(), 2.0)].into(),
        );
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
