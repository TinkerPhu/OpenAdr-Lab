use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use lab_core::time_window::TimeWindow;

/// Capacity state in force now, derived from the listed OpenADR events
/// (`controller::openadr_interface::parse_capacity_state`, GB-48).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OadrCapacityState {
    /// Import limit in force at the last poll (kW); None = no limit now. A
    /// limit scheduled for later is in `CapacitySnapshot`s, not here.
    pub import_limit_kw: Option<f64>,
    /// Export limit in force at the last poll (kW); None = no limit now.
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

/// One segment of the capacity-limit schedule, mirroring `TariffSnapshot`'s
/// shape. Parsed from IMPORT_CAPACITY_LIMIT/EXPORT_CAPACITY_LIMIT event
/// payloads (the OpenADR 3.1 "Dynamic Operating Envelope", User Guide §8.10.1),
/// priority-resolved and non-overlapping. The schedule is the single source
/// for which limit applies when — read it through `tightest_capacity_limit`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapacitySnapshot {
    pub interval_start: DateTime<Utc>,
    #[serde(skip_serializing)]
    pub interval_end: DateTime<Utc>,
    pub import_limit_kw: Option<f64>,
    pub export_limit_kw: Option<f64>,
    /// The event each limit came from (the priority winner for this segment).
    #[serde(default)]
    pub import_limit_event_id: Option<String>,
    #[serde(default)]
    pub export_limit_event_id: Option<String>,
}

/// A capacity limit and the event it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct CapacityLimit {
    pub limit_kw: f64,
    pub event_id: Option<String>,
}

/// Which capacity limit applies (GB-48) — the one answer for the planner's
/// slot caps, the limit in force now (`OadrCapacityState`), the history
/// sampler and the arbiter's limit pass: the tightest `direction` limit among
/// `schedule` segments overlapping `[from, to)`. For `from == to` it is the
/// instant `from`, covered by segments with `start ≤ from < end`.
pub fn tightest_capacity_limit(
    schedule: &[CapacitySnapshot],
    direction: crate::entities::capacity_curve::CommitmentDirection,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Option<CapacityLimit> {
    use crate::entities::capacity_curve::CommitmentDirection::{Export, Import};
    schedule
        .iter()
        .filter(|s| s.overlaps(from, to))
        .filter_map(|s| {
            let (limit_kw, event_id) = match direction {
                Import => (s.import_limit_kw?, &s.import_limit_event_id),
                Export => (s.export_limit_kw?, &s.export_limit_event_id),
            };
            Some(CapacityLimit {
                limit_kw,
                event_id: event_id.clone(),
            })
        })
        .min_by(|a, b| a.limit_kw.total_cmp(&b.limit_kw))
}

/// WP3.1 (BL-04) — an active grid-alert window parsed from an
/// ALERT_GRID_EMERGENCY / ALERT_BLACK_START event. Both alert types carry a
/// human-readable string payload (Definition doc, event payload type table);
/// the window is the interval's, per the shared timing rule
/// (`controller::event_timing`).
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
    /// How long one report interval covers -- the bucket width.
    ///
    /// Distinct from `submit_every_s`: 3.1's `frequency` counts intervals
    /// *between* reports, so a 60 s grid with `frequency: 4` means four 60 s
    /// intervals carried by one submission every 240 s. These were a single
    /// number, which made that case report one 240 s bucket -- coarser data
    /// than the VTN asked for, arriving when it asked for it.
    pub interval_width_s: u64,
    /// How long until the next submission is due.
    pub submit_every_s: u64,
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

// All four carry a half-open `[start, end)` span, so they answer "in force at
// t?", "overlapping this slot?" and "over?" through the one shared rule
// (`entities::time_window`). WP4.6 review fix, still the reason `is_ended`
// matters: OpenADR events are permanent records — an ended window stays in
// state as long as its event exists on the VTN, so consumers that show
// "current" signals (GET /signals, the UI strip) must drop ended windows.
impl TimeWindow for AlertWindow {
    fn start(&self) -> DateTime<Utc> {
        self.start
    }
    fn end(&self) -> DateTime<Utc> {
        self.end
    }
}

impl TimeWindow for SimpleWindow {
    fn start(&self) -> DateTime<Utc> {
        self.start
    }
    fn end(&self) -> DateTime<Utc> {
        self.end
    }
}

impl TimeWindow for DispatchWindow {
    fn start(&self) -> DateTime<Utc> {
        self.start
    }
    fn end(&self) -> DateTime<Utc> {
        self.end
    }
}

impl TimeWindow for CapacitySnapshot {
    fn start(&self) -> DateTime<Utc> {
        self.interval_start
    }
    fn end(&self) -> DateTime<Utc> {
        self.interval_end
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

#[cfg(test)]
mod capacity_limit_tests {
    use super::*;
    use crate::entities::capacity_curve::CommitmentDirection::{Export, Import};
    use chrono::TimeZone;

    fn at(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 15, h, m, 0).unwrap()
    }

    fn seg(
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        imp: Option<f64>,
        exp: Option<f64>,
    ) -> CapacitySnapshot {
        CapacitySnapshot {
            interval_start: from,
            interval_end: to,
            import_limit_kw: imp,
            export_limit_kw: exp,
            import_limit_event_id: imp.map(|_| format!("imp-{}", from.format("%H%M"))),
            export_limit_event_id: exp.map(|_| format!("exp-{}", from.format("%H%M"))),
        }
    }

    #[test]
    fn tightest_capacity_limit_at_an_instant_inside_a_segment() {
        let schedule = [seg(at(10, 0), at(11, 0), Some(3.0), None)];
        let limit = tightest_capacity_limit(&schedule, Import, at(10, 30), at(10, 30)).unwrap();
        assert_eq!(limit.limit_kw, 3.0);
        assert_eq!(limit.event_id.as_deref(), Some("imp-1000"));
    }

    #[test]
    fn tightest_capacity_limit_instant_is_half_open() {
        let schedule = [seg(at(10, 0), at(11, 0), Some(3.0), None)];
        assert!(tightest_capacity_limit(&schedule, Import, at(10, 0), at(10, 0)).is_some());
        assert!(tightest_capacity_limit(&schedule, Import, at(11, 0), at(11, 0)).is_none());
        assert!(tightest_capacity_limit(&schedule, Import, at(9, 59), at(9, 59)).is_none());
    }

    #[test]
    fn tightest_capacity_limit_over_a_span_takes_the_tightest_overlap() {
        let schedule = [
            seg(at(10, 0), at(10, 40), Some(5.0), None),
            seg(at(10, 40), at(11, 0), Some(2.0), None),
        ];
        let limit = tightest_capacity_limit(&schedule, Import, at(10, 0), at(11, 0)).unwrap();
        assert_eq!(limit.limit_kw, 2.0);
        // A span that ends where the tighter segment starts does not overlap it.
        let limit = tightest_capacity_limit(&schedule, Import, at(10, 0), at(10, 40)).unwrap();
        assert_eq!(limit.limit_kw, 5.0);
    }

    #[test]
    fn tightest_capacity_limit_reads_the_requested_direction() {
        let schedule = [seg(at(10, 0), at(11, 0), Some(3.0), Some(0.5))];
        let exp = tightest_capacity_limit(&schedule, Export, at(10, 0), at(11, 0)).unwrap();
        assert_eq!(
            (exp.limit_kw, exp.event_id.as_deref()),
            (0.5, Some("exp-1000"))
        );
        let only_imp = [seg(at(10, 0), at(11, 0), Some(3.0), None)];
        assert!(tightest_capacity_limit(&only_imp, Export, at(10, 0), at(11, 0)).is_none());
    }

    // Moved from the GB-47 limit pass's own lookup (`capacity_import_limit_at_kw`,
    // now removed): a limit scheduled for later is not in force now, and
    // overlapping segments at an instant give the tightest.
    #[test]
    fn tightest_capacity_limit_ignores_later_limits_and_takes_the_tightest_overlap() {
        let schedule = [
            seg(at(9, 55), at(10, 5), Some(3.0), None),
            seg(at(10, 10), at(10, 20), Some(1.0), None),
        ];
        let now = at(10, 0);
        assert_eq!(
            tightest_capacity_limit(&schedule, Import, now, now)
                .unwrap()
                .limit_kw,
            3.0
        );
        assert!(tightest_capacity_limit(&schedule[1..], Import, now, now).is_none());
        let overlapping = [
            seg(at(9, 55), at(10, 5), Some(3.0), None),
            seg(at(9, 59), at(10, 1), Some(2.0), None),
        ];
        assert_eq!(
            tightest_capacity_limit(&overlapping, Import, now, now)
                .unwrap()
                .limit_kw,
            2.0
        );
    }

    #[test]
    fn tightest_capacity_limit_none_without_overlap() {
        let schedule = [seg(at(10, 0), at(11, 0), Some(3.0), None)];
        assert!(tightest_capacity_limit(&schedule, Import, at(12, 0), at(13, 0)).is_none());
        assert!(tightest_capacity_limit(&[], Import, at(10, 0), at(11, 0)).is_none());
    }
}
