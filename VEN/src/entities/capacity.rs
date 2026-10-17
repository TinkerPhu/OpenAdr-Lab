use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::entities::capacity_curve::CommitmentDirection;
use lab_core::time_window::TimeWindow;

/// The capacity state in force at one moment — the API view (`GET /capacity`, `GET /signals`) of the
/// capacity schedule, built by `OadrCapacityState::at` when asked and never stored (GB-48): every
/// value is the schedule read at that moment's own time, so it changes at its interval boundary and
/// not at the next event poll.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default, TS)]
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

impl OadrCapacityState {
    /// The capacity state in force at `now`, read from `schedule`: the limits through
    /// `effective_capacity_limit` (a VTN limit, else a sim-injected stand-in), the subscription
    /// and reservation of the segment covering `now`. `last_updated` is the last event poll that
    /// listed capacity payloads — a fact about the poll, which the schedule does not carry.
    pub fn at(
        schedule: &[CapacitySnapshot],
        inject: &crate::entities::sim_inject::SimInjectState,
        now: DateTime<Utc>,
        last_updated: Option<DateTime<Utc>>,
    ) -> Self {
        let limit = |direction| effective_capacity_limit(schedule, inject, direction, now, now);
        let at_now = |value: fn(&CapacitySnapshot) -> Option<f64>| {
            tightest_over(schedule, now, now, |s| {
                value(s).map(|limit_kw| CapacityLimit {
                    limit_kw,
                    event_id: None,
                })
            })
            .map(|l| l.limit_kw)
        };
        let (import, export) = (
            limit(CommitmentDirection::Import),
            limit(CommitmentDirection::Export),
        );
        Self {
            import_limit_kw: import.as_ref().map(|l| l.limit_kw),
            export_limit_kw: export.as_ref().map(|l| l.limit_kw),
            import_subscription_kw: at_now(|s| s.import_subscription_kw),
            import_reservation_kw: at_now(|s| s.import_reservation_kw),
            export_subscription_kw: at_now(|s| s.export_subscription_kw),
            export_reservation_kw: at_now(|s| s.export_reservation_kw),
            import_limit_event_id: import.and_then(|l| l.event_id),
            export_limit_event_id: export.and_then(|l| l.event_id),
            last_updated,
        }
    }
}

/// The six capacity payload types the schedule carries, declared once: the
/// parser collects exactly these, and nothing else lists them
/// (`controller::rate_schedule::parse_capacity_schedule`).
pub const CAPACITY_PAYLOAD_TYPES: [&str; 6] = [
    "IMPORT_CAPACITY_LIMIT",
    "EXPORT_CAPACITY_LIMIT",
    "IMPORT_CAPACITY_SUBSCRIPTION",
    "EXPORT_CAPACITY_SUBSCRIPTION",
    "IMPORT_CAPACITY_RESERVATION",
    "EXPORT_CAPACITY_RESERVATION",
];

/// One segment of the capacity schedule, mirroring `TariffSnapshot`'s shape.
/// Parsed from the `*_CAPACITY_LIMIT` / `*_SUBSCRIPTION` / `*_RESERVATION` event
/// payloads (the OpenADR 3.1 "Dynamic Operating Envelope" and capacity
/// management, User Guide §8.10), priority-resolved and non-overlapping. The
/// schedule is the single source for which capacity value applies when — read it
/// through `tightest_capacity_limit` / `contracted_allowance_kw`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
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
    /// Subscribed capacity (kW) committed to the grid for this segment.
    #[serde(default)]
    pub import_subscription_kw: Option<f64>,
    #[serde(default)]
    pub export_subscription_kw: Option<f64>,
    /// Capacity reservation granted by the VTN for this segment (kW).
    #[serde(default)]
    pub import_reservation_kw: Option<f64>,
    #[serde(default)]
    pub export_reservation_kw: Option<f64>,
}

impl CapacitySnapshot {
    /// This segment's limit in `direction` and the event it came from.
    fn limit(&self, direction: CommitmentDirection) -> Option<CapacityLimit> {
        let (limit_kw, event_id) = match direction {
            CommitmentDirection::Import => (self.import_limit_kw?, &self.import_limit_event_id),
            CommitmentDirection::Export => (self.export_limit_kw?, &self.export_limit_event_id),
        };
        Some(CapacityLimit {
            limit_kw,
            event_id: event_id.clone(),
        })
    }

    /// This segment's contracted allowance in `direction`: subscription plus
    /// reservation, either alone counting (WP3.3, User Guide §8.10.2 — a
    /// reservation raises the allowance above the subscription); `None` when the
    /// segment declares neither.
    fn allowance(&self, direction: CommitmentDirection) -> Option<CapacityLimit> {
        let (subscription_kw, reservation_kw) = match direction {
            CommitmentDirection::Import => {
                (self.import_subscription_kw, self.import_reservation_kw)
            }
            CommitmentDirection::Export => {
                (self.export_subscription_kw, self.export_reservation_kw)
            }
        };
        (subscription_kw.is_some() || reservation_kw.is_some()).then(|| CapacityLimit {
            limit_kw: subscription_kw.unwrap_or(0.0) + reservation_kw.unwrap_or(0.0),
            event_id: None,
        })
    }
}

/// A capacity value and the event it came from (`None` for an allowance, which
/// combines two events' payloads, and for a sim-injected stand-in).
#[derive(Debug, Clone, PartialEq)]
pub struct CapacityLimit {
    pub limit_kw: f64,
    pub event_id: Option<String>,
}

/// The one primitive behind every "which capacity value applies" question: the
/// tightest value `pick` reads among `schedule` segments overlapping `[from, to)`.
/// For `from == to` it is the instant `from`, covered by segments with
/// `start ≤ from < end`. Limits and the contracted allowance are two `pick`s.
fn tightest_over(
    schedule: &[CapacitySnapshot],
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    pick: impl Fn(&CapacitySnapshot) -> Option<CapacityLimit>,
) -> Option<CapacityLimit> {
    schedule
        .iter()
        .filter(|s| s.overlaps(from, to))
        .filter_map(pick)
        .min_by(|a, b| a.limit_kw.total_cmp(&b.limit_kw))
}

/// Which capacity limit applies (GB-48) — the one answer for the planner's
/// slot caps, the grid asset, the PV generation-limit resolver, the history
/// sampler, the arbiter's limit pass and `GET /capacity`: the tightest
/// `direction` limit among `schedule` segments overlapping `[from, to)`.
pub fn tightest_capacity_limit(
    schedule: &[CapacitySnapshot],
    direction: CommitmentDirection,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Option<CapacityLimit> {
    tightest_over(schedule, from, to, |s| s.limit(direction))
}

/// The contracted allowance (kW) over `[from, to)`: the tightest per-segment
/// subscription + reservation among overlapping segments, or `INFINITY` when no
/// segment declares one.
///
/// `INFINITY` is the honest "uncapped" value, not a sentinel: it makes the
/// `.min(allowance)` at the planner's slot cap and the "is anything binding?"
/// question in `ReservationRequest` both come out right without either caller
/// special-casing `None`. Where no limit covers a slot but a subscription does,
/// this is what caps it — the spec's default for an exhausted limit schedule
/// (User Guide §8.10.1).
pub fn contracted_allowance_kw(
    schedule: &[CapacitySnapshot],
    direction: CommitmentDirection,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> f64 {
    tightest_over(schedule, from, to, |s| s.allowance(direction))
        .map_or(f64::INFINITY, |a| a.limit_kw)
}

/// The limit to enforce over `[from, to)`: the schedule's VTN limit, else the
/// sim-injected one. A manual limit substitutes for a VTN directive where none
/// applies and never overrides one — the only home of that rule.
pub fn effective_capacity_limit(
    schedule: &[CapacitySnapshot],
    inject: &crate::entities::sim_inject::SimInjectState,
    direction: CommitmentDirection,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Option<CapacityLimit> {
    tightest_capacity_limit(schedule, direction, from, to).or_else(|| {
        let limit_kw = match direction {
            CommitmentDirection::Import => inject.grid_import_limit_kw,
            CommitmentDirection::Export => inject.grid_export_limit_kw,
        }?;
        Some(CapacityLimit {
            limit_kw,
            event_id: None,
        })
    })
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct DispatchWindow {
    pub setpoint_kw: f64,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub event_id: String,
}

/// Pending report obligation derived from OpenADR event's reportDescriptors (§5.3).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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
    /// From `reportDescriptor.aggregate` (spec default false): true = one result for all the
    /// VEN's resources; false = one result per resource (R-143). Honoured by the device
    /// payloads (state of charge); site-meter payloads have one resource either way.
    #[serde(default)]
    pub aggregate: bool,
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
            ..CapacitySnapshot::default()
        }
    }

    fn allowance_seg(
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        subscription_kw: Option<f64>,
        reservation_kw: Option<f64>,
    ) -> CapacitySnapshot {
        CapacitySnapshot {
            interval_start: from,
            interval_end: to,
            import_subscription_kw: subscription_kw,
            import_reservation_kw: reservation_kw,
            ..CapacitySnapshot::default()
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

    #[test]
    fn contracted_allowance_sums_subscription_and_reservation_per_segment() {
        let schedule = [allowance_seg(at(10, 0), at(11, 0), Some(5.0), Some(3.0))];
        assert_eq!(
            contracted_allowance_kw(&schedule, Import, at(10, 0), at(11, 0)),
            8.0
        );
        // Either one counts alone.
        let sub_only = [allowance_seg(at(10, 0), at(11, 0), Some(5.0), None)];
        assert_eq!(
            contracted_allowance_kw(&sub_only, Import, at(10, 0), at(11, 0)),
            5.0
        );
        let res_only = [allowance_seg(at(10, 0), at(11, 0), None, Some(3.0))];
        assert_eq!(
            contracted_allowance_kw(&res_only, Import, at(10, 0), at(11, 0)),
            3.0
        );
    }

    #[test]
    fn contracted_allowance_is_uncapped_where_no_segment_declares_one() {
        let schedule = [allowance_seg(at(10, 0), at(11, 0), Some(5.0), None)];
        assert_eq!(
            contracted_allowance_kw(&schedule, Import, at(12, 0), at(13, 0)),
            f64::INFINITY
        );
        // A segment carrying only a limit declares no allowance.
        let limit_only = [seg(at(10, 0), at(11, 0), Some(3.0), None)];
        assert_eq!(
            contracted_allowance_kw(&limit_only, Import, at(10, 0), at(11, 0)),
            f64::INFINITY
        );
        assert_eq!(
            contracted_allowance_kw(&[], Export, at(10, 0), at(11, 0)),
            f64::INFINITY
        );
    }

    #[test]
    fn a_future_reservation_does_not_touch_an_earlier_span() {
        let schedule = [
            allowance_seg(at(0, 0), at(12, 0), Some(5.0), None),
            allowance_seg(at(12, 0), at(14, 0), Some(5.0), Some(3.0)),
        ];
        assert_eq!(
            contracted_allowance_kw(&schedule, Import, at(10, 0), at(11, 0)),
            5.0
        );
        assert_eq!(
            contracted_allowance_kw(&schedule, Import, at(12, 0), at(13, 0)),
            8.0
        );
        // A span straddling the start takes the tightest overlap, like a limit.
        assert_eq!(
            contracted_allowance_kw(&schedule, Import, at(11, 30), at(12, 30)),
            5.0
        );
    }

    #[test]
    fn contracted_allowance_reads_the_requested_direction() {
        let schedule = [CapacitySnapshot {
            interval_start: at(10, 0),
            interval_end: at(11, 0),
            export_subscription_kw: Some(4.0),
            export_reservation_kw: Some(2.0),
            ..CapacitySnapshot::default()
        }];
        assert_eq!(
            contracted_allowance_kw(&schedule, Export, at(10, 0), at(11, 0)),
            6.0
        );
        assert_eq!(
            contracted_allowance_kw(&schedule, Import, at(10, 0), at(11, 0)),
            f64::INFINITY
        );
    }

    fn inject(
        import_kw: Option<f64>,
        export_kw: Option<f64>,
    ) -> crate::entities::sim_inject::SimInjectState {
        crate::entities::sim_inject::SimInjectState {
            grid_import_limit_kw: import_kw,
            grid_export_limit_kw: export_kw,
            ..Default::default()
        }
    }

    #[test]
    fn an_injected_limit_stands_in_where_no_vtn_limit_applies() {
        let now = at(10, 0);
        let out = effective_capacity_limit(&[], &inject(Some(7.0), Some(3.0)), Import, now, now);
        assert_eq!(out.map(|l| l.limit_kw), Some(7.0));
        let out = effective_capacity_limit(&[], &inject(Some(7.0), Some(3.0)), Export, now, now);
        assert_eq!(out.map(|l| l.limit_kw), Some(3.0));
    }

    #[test]
    fn a_vtn_limit_is_never_overridden_by_an_injected_one() {
        let now = at(10, 0);
        let schedule = [seg(at(9, 0), at(11, 0), Some(5.0), None)];
        let out = effective_capacity_limit(&schedule, &inject(Some(7.0), None), Import, now, now);
        let out = out.unwrap();
        assert_eq!(
            (out.limit_kw, out.event_id.as_deref()),
            (5.0, Some("imp-0900"))
        );
    }

    #[test]
    fn each_direction_is_decided_on_its_own() {
        // An import limit in force must not stop an injected *export* limit.
        let now = at(10, 0);
        let schedule = [seg(at(9, 0), at(11, 0), Some(5.0), None)];
        let inj = inject(Some(7.0), Some(3.0));
        assert_eq!(
            effective_capacity_limit(&schedule, &inj, Import, now, now).map(|l| l.limit_kw),
            Some(5.0)
        );
        assert_eq!(
            effective_capacity_limit(&schedule, &inj, Export, now, now).map(|l| l.limit_kw),
            Some(3.0)
        );
    }

    #[test]
    fn no_injection_and_no_vtn_limit_means_no_limit() {
        let now = at(10, 0);
        assert!(effective_capacity_limit(&[], &inject(None, None), Import, now, now).is_none());
    }

    #[test]
    fn the_view_reads_every_quantity_at_its_own_time() {
        let schedule = [
            CapacitySnapshot {
                import_limit_kw: Some(4.0),
                import_limit_event_id: Some("lim".into()),
                import_subscription_kw: Some(5.0),
                import_reservation_kw: Some(3.0),
                export_subscription_kw: Some(2.0),
                ..seg(at(10, 0), at(11, 0), None, None)
            },
            seg(at(11, 0), at(12, 0), Some(1.0), None),
        ];
        let inject = crate::entities::sim_inject::SimInjectState::default();
        let view = |t| OadrCapacityState::at(&schedule, &inject, t, None);
        let first = view(at(10, 30));
        assert_eq!(first.import_limit_kw, Some(4.0));
        assert_eq!(first.import_limit_event_id.as_deref(), Some("lim"));
        assert_eq!(first.import_subscription_kw, Some(5.0));
        assert_eq!(first.import_reservation_kw, Some(3.0));
        assert_eq!(first.export_subscription_kw, Some(2.0));
        assert_eq!(first.export_limit_kw, None);
        // The next interval: its own limit, and no subscription or reservation.
        let second = view(at(11, 0));
        assert_eq!(second.import_limit_kw, Some(1.0));
        assert_eq!(second.import_subscription_kw, None);
        assert_eq!(second.import_reservation_kw, None);
        // Before and after the schedule: nothing is in force.
        assert_eq!(view(at(9, 59)).import_limit_kw, None);
        assert_eq!(view(at(12, 0)).import_limit_kw, None);
    }

    #[test]
    fn the_view_shows_an_injected_limit_without_an_event_id_where_no_vtn_limit_applies() {
        let inject = inject(Some(7.0), None);
        let view = OadrCapacityState::at(&[], &inject, at(10, 0), Some(at(9, 0)));
        assert_eq!(view.import_limit_kw, Some(7.0));
        assert_eq!(view.import_limit_event_id, None);
        assert_eq!(view.last_updated, Some(at(9, 0)));
    }
}
