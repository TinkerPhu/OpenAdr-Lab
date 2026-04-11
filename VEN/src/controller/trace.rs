/// Controller observability: ControllerEvent log.
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
        total_slots: usize,
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
// ControllerTrace — event log holder
// ---------------------------------------------------------------------------

/// Controller observability state: event log.
/// Per-asset history is now owned by each `AssetEntry.history` in `SimState`.
#[derive(Debug, Clone)]
pub struct ControllerTrace {
    pub event_log: ControllerEventLog,
}

impl ControllerTrace {
    pub fn new() -> Self {
        Self {
            event_log: ControllerEventLog::new(500),
        }
    }

    pub fn push_event(&mut self, event: ControllerEvent) {
        self.event_log.push(event);
    }

    pub fn events(&self) -> Vec<ControllerEvent> {
        self.event_log.entries()
    }
}

impl Default for ControllerTrace {
    fn default() -> Self {
        Self::new()
    }
}
