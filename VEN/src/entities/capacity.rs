// Module-wide allow: `OadrCapacityState`/`OadrReportObligation` are live (constructed via
// serde deserialization and read elsewhere); `OadrProgramConfig`/`OadrEventCache`/
// `OadrCapacityRequest` below are unwired sketches, kept intentionally (not deleted) —
// see docs/BACKLOG.md BL-24 and docs/reference/TECHNICAL_DEBTS.md R-13 (DISPATCH_SETPOINT).
#![allow(dead_code)]
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::site_meter::PowerSnapshot;
use crate::entities::tariff_snapshot::TariffSnapshot;

/// Current capacity state derived from active OpenADR events.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OadrCapacityState {
    /// Maximum import power allowed (kW); None = no limit
    pub import_limit_kw: Option<f64>,
    /// Maximum export power allowed (kW); None = no limit
    pub export_limit_kw: Option<f64>,
    /// Subscribed capacity (kW) committed to the grid
    pub import_subscription_kw: Option<f64>,
    /// Active import reservation granted by VTN (kW)
    pub import_reservation_kw: Option<f64>,
    /// WP3.3: subscribed export capacity (kW); None = not set
    #[serde(default)]
    pub export_subscription_kw: Option<f64>,
    /// WP3.3: active export reservation granted by VTN (kW); None = not set
    #[serde(default)]
    pub export_reservation_kw: Option<f64>,
    /// Source event ID for the import limit
    pub import_limit_event_id: Option<String>,
    /// Source event ID for the export limit
    pub export_limit_event_id: Option<String>,
    pub last_updated: Option<DateTime<Utc>>,
}

/// WP3.1 (BL-04) — an active grid-alert window parsed from an
/// ALERT_GRID_EMERGENCY / ALERT_BLACK_START event. Both alert types carry a
/// human-readable string payload (Definition doc, event payload type table)
/// and take their window from the interval's own `intervalPeriod`, falling
/// back to the event-level one (User Guide Example 8.1-1 uses the latter).
/// Both mean "minimize electricity use": the planner clamps the contractual
/// import cap to 0 over the window (soft constraint — unavoidable base load
/// becomes a penalized violation with a PlanWarning, never infeasibility);
/// export is left untouched since the spec prescribes nothing for it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlertWindow {
    pub alert_type: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub event_id: String,
    pub message: String,
}

/// WP3.2 — a SIMPLE load-shed window parsed from a SIMPLE event (levels
/// 0–3, User Guide §8.2). Level semantics in this lab (decision recorded
/// here; percentages profile-configurable where noted):
///   0 = normal (window dropped at parse time),
///   1 = mild — import cap clamped to `simple_level1_import_cap_pct` ×
///       contractual limit (planner profile key, default 50%),
///   2 = moderate — import cap clamped to the slot's baseline forecast, so
///       all FLEXIBLE/OPPORTUNISTIC consumption above uncontrollable load is
///       deferred (comfort-critical loads may still exceed via the soft
///       violation slack, penalized + warned),
///   3 = severe — import cap 0, same as the WP3.1 alert path.
/// Overlaps: highest level wins per slot; alert windows override everything.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimpleWindow {
    pub level: u8,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub event_id: String,
}

/// WP3.4 (BL-06/BL-24) — a direct-dispatch window parsed from a
/// DISPATCH_SETPOINT event: while active, the dispatcher applies the
/// commanded net site setpoint (kW, positive = import; lab convention
/// matching every other capacity payload) directly by steering the battery,
/// bypassing the plan. The planner keeps running; normal plan-following
/// resumes when the window ends. Precedence (decision): an active alert
/// window wins over dispatch — safety over instruction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DispatchWindow {
    pub setpoint_kw: f64,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub event_id: String,
}

/// Configuration for an OpenADR program this VEN participates in (§5.1).
/// Unwired sketch — never constructed anywhere. See docs/BACKLOG.md BL-24.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OadrProgramConfig {
    pub program_id: String,
    pub program_name: String,
    /// Signal types this program sends (e.g. ["PRICE", "GHG", "EXPORT_PRICE"])
    pub payload_types: Vec<String>,
    /// Report types VTN expects from us (e.g. ["USAGE", "DEMAND"])
    pub report_types: Vec<String>,
    pub currency: Option<String>,  // e.g. "EUR"
    pub units: Option<String>,     // e.g. "KWH"
    pub is_capacity_program: bool, // participates in capacity management
}

/// Internal representation of a received OpenADR event, translated into domain terms (§5.2).
/// Unwired sketch — never constructed anywhere; `dispatch_setpoints` is the storage this
/// would need once DISPATCH_SETPOINT parsing exists. See docs/BACKLOG.md BL-24 and
/// docs/reference/TECHNICAL_DEBTS.md R-13.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OadrEventCache {
    pub event_id: String,
    pub program_id: String,
    pub event_name: Option<String>,
    pub received_at: Option<DateTime<Utc>>,

    // Translated content
    pub rate_snapshots: Vec<TariffSnapshot>, // PRICE, EXPORT_PRICE, GHG per interval
    pub capacity_limits: Vec<TariffSnapshot>, // IMPORT/EXPORT_CAPACITY_LIMIT per interval
    pub alert_type: Option<String>,          // e.g. "ALERT_GRID_EMERGENCY"
    pub alert_message: Option<String>,
    pub dispatch_setpoints: Vec<PowerSnapshot>, // DISPATCH_SETPOINT per interval

    pub raw: serde_json::Value, // original OpenADR event JSON
}

/// Pending report obligation derived from OpenADR event's reportDescriptors (§5.3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OadrReportObligation {
    pub id: Uuid,
    pub event_id: String,
    pub program_id: Option<String>,
    /// e.g. "USAGE", "DEMAND", "USAGE_FORECAST", "STORAGE_CHARGE_STATE"
    pub payload_type: String,
    /// e.g. "DIRECT_READ", "FORECAST"
    pub reading_type: String,
    pub resource_name: Option<String>,
    pub due_at: DateTime<Utc>,
    pub interval_duration_s: u64,
    pub fulfilled: bool,
    pub created_at: DateTime<Utc>,
    /// From `reportDescriptor.historical` (spec default true): true = report
    /// past data; false = the VTN asked for a forecast (R-15).
    pub historical: bool,
}

impl OadrReportObligation {
    /// True if the obligation is unfulfilled and its due time has passed.
    pub fn is_due(&self, now: DateTime<Utc>) -> bool {
        !self.fulfilled && now >= self.due_at
    }
}

/// A capacity reservation request sent to the VTN (Stage 2+).
/// Unwired sketch — never constructed anywhere. See docs/BACKLOG.md BL-24.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OadrCapacityRequest {
    pub program_id: String,
    pub requested_import_kw: Option<f64>,
    pub requested_export_kw: Option<f64>,
    pub time_window_start: DateTime<Utc>,
    pub time_window_end: DateTime<Utc>,
    pub reason: String,
}

/// WP4.6 review fix: OpenADR events are permanent records — an ended window
/// stays in state as long as its event exists on the VTN. Consumers that show
/// "current" signals (GET /signals, the UI strip) must drop ended windows;
/// the planner/dispatcher already filter by slot/now overlap themselves.
impl AlertWindow {
    pub fn is_ended(&self, now: DateTime<Utc>) -> bool {
        now >= self.end
    }
}

impl SimpleWindow {
    pub fn is_ended(&self, now: DateTime<Utc>) -> bool {
        now >= self.end
    }
}

impl DispatchWindow {
    pub fn is_ended(&self, now: DateTime<Utc>) -> bool {
        now >= self.end
    }
}

#[cfg(test)]
mod window_expiry_tests {
    use super::*;
    use chrono::TimeZone;

    fn ts(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000 + secs, 0).unwrap()
    }

    #[test]
    fn test_is_ended_boundary_semantics() {
        let w = AlertWindow {
            alert_type: "ALERT_GRID_EMERGENCY".into(),
            start: ts(0),
            end: ts(600),
            event_id: "e".into(),
            message: String::new(),
        };
        assert!(!w.is_ended(ts(0)), "active at start");
        assert!(!w.is_ended(ts(599)), "active until the last second");
        assert!(w.is_ended(ts(600)), "ended exactly at end (end-exclusive)");
        let s = SimpleWindow {
            level: 2,
            start: ts(0),
            end: ts(600),
            event_id: "e".into(),
        };
        assert!(s.is_ended(ts(601)));
        let d = DispatchWindow {
            setpoint_kw: 2.0,
            start: ts(0),
            end: ts(600),
            event_id: "e".into(),
        };
        assert!(!d.is_ended(ts(300)));
    }
}
