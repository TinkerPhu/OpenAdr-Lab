use chrono::{DateTime, Duration, Utc};
use tracing::debug;
use uuid::Uuid;

use crate::controller::event_timing::{timed_intervals, TimedInterval};
use crate::controller::vtn_port::{EventTypeName, OadrEvent, OadrPayload, PayloadValues};
use crate::entities::capacity::{
    AlertWindow, DispatchWindow, OadrCapacityState, OadrReportObligation, SimpleWindow,
};
use openleadr_wire::report::ReportDescriptor as WireReportDescriptor;

// Rate/capacity-schedule parsing lives in `rate_schedule.rs` (split out to stay under the
// VEN/src/ 500-production-line cap) — re-exported here so call sites and this file's own
// tests can keep referencing `openadr_interface::{parse_rate_snapshots, parse_capacity_schedule}`.
pub use crate::controller::rate_schedule::{parse_capacity_schedule, parse_rate_snapshots};

// ---------------------------------------------------------------------------
// Capacity state parsing
// ---------------------------------------------------------------------------

/// The capacity state in force at `now` (GB-48). The import/export limits are
/// the priority-resolved schedule's values covering `now`, read through the one
/// lookup (`entities::capacity::tightest_capacity_limit`) — a limit that has not
/// started or has ended is not in force. Subscription and reservation stay the
/// strictest value over all listed events (making them time-aware is a separate
/// step, BACKLOG GB-48 option C). `last_updated` is set when any capacity payload
/// is listed.
pub fn parse_capacity_state(events: &[OadrEvent], now: DateTime<Utc>) -> OadrCapacityState {
    use crate::entities::capacity::tightest_capacity_limit;
    use crate::entities::capacity_curve::CommitmentDirection::{Export, Import};

    let strictest = |payload_type: &str| {
        events
            .iter()
            .flat_map(|e| e.content.intervals.iter().flatten())
            .flat_map(|i| &i.payloads)
            .filter(|p| p.value_type.wire_name() == payload_type)
            .filter_map(|p| p.numeric())
            .reduce(f64::min)
    };
    let found_any = [
        "IMPORT_CAPACITY_LIMIT",
        "EXPORT_CAPACITY_LIMIT",
        "IMPORT_CAPACITY_SUBSCRIPTION",
        "IMPORT_CAPACITY_RESERVATION",
        "EXPORT_CAPACITY_SUBSCRIPTION",
        "EXPORT_CAPACITY_RESERVATION",
    ]
    .iter()
    .any(|t| strictest(t).is_some());
    if !found_any {
        return OadrCapacityState::default();
    }

    let schedule = parse_capacity_schedule(events);
    let import = tightest_capacity_limit(&schedule, Import, now, now);
    let export = tightest_capacity_limit(&schedule, Export, now, now);
    OadrCapacityState {
        import_limit_kw: import.as_ref().map(|l| l.limit_kw),
        import_limit_event_id: import.and_then(|l| l.event_id),
        export_limit_kw: export.as_ref().map(|l| l.limit_kw),
        export_limit_event_id: export.and_then(|l| l.event_id),
        import_subscription_kw: strictest("IMPORT_CAPACITY_SUBSCRIPTION"),
        import_reservation_kw: strictest("IMPORT_CAPACITY_RESERVATION"),
        export_subscription_kw: strictest("EXPORT_CAPACITY_SUBSCRIPTION"),
        export_reservation_kw: strictest("EXPORT_CAPACITY_RESERVATION"),
        last_updated: Some(now),
    }
}

// ---------------------------------------------------------------------------
// Windowed signals: alerts, SIMPLE levels, direct setpoints
// ---------------------------------------------------------------------------

/// Every payload of one of `payload_types` in `events`, with its event and its
/// interval's window. Interval timing is the one shared rule
/// (`controller::event_timing`): contiguous intervals from the event-level
/// period, an interval's own period winning, open-ended without a duration,
/// in force while listed without any start (GB-48).
fn timed_payloads<'a>(
    events: &'a [OadrEvent],
    payload_types: &'a [&'a str],
) -> impl Iterator<Item = (&'a OadrEvent, TimedInterval<'a>, &'a OadrPayload)> + 'a {
    events.iter().flat_map(move |event| {
        timed_intervals(event).into_iter().flat_map(move |timed| {
            timed
                .interval
                .payloads
                .iter()
                .filter(move |p| payload_types.contains(&p.value_type.wire_name().as_str()))
                .map(move |p| (event, timed, p))
        })
    })
}

/// Grid-alert windows (ALERT_GRID_EMERGENCY / ALERT_BLACK_START, WP3.1/BL-04).
/// The payload value is the spec's human-readable message.
pub fn parse_alert_windows(events: &[OadrEvent]) -> Vec<AlertWindow> {
    timed_payloads(events, &["ALERT_GRID_EMERGENCY", "ALERT_BLACK_START"])
        .map(|(event, timed, payload)| AlertWindow {
            alert_type: payload.value_type.wire_name(),
            start: timed.start,
            end: timed.end,
            event_id: event.id.to_string(),
            message: payload.text().unwrap_or("").to_string(),
        })
        .collect()
}

/// SIMPLE load-shed windows, levels 1–3 (WP3.2). Level 0 ("normal") windows
/// are dropped — they constrain nothing. Non-numeric or out-of-range values
/// are skipped.
pub fn parse_simple_windows(events: &[OadrEvent]) -> Vec<SimpleWindow> {
    timed_payloads(events, &["SIMPLE"])
        .filter_map(|(event, timed, payload)| {
            // `numeric()` deliberately, not a `Number` match: SIMPLE's
            // declared value kind is `Integer`, so matching only `Number`
            // would drop every load-shed level silently.
            let level = payload.numeric().filter(|v| (1.0..=3.0).contains(v))? as u8;
            Some(SimpleWindow {
                level,
                start: timed.start,
                end: timed.end,
                event_id: event.id.to_string(),
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Direct setpoints (WP3.4 — BL-06/BL-24)
// ---------------------------------------------------------------------------

/// DISPATCH_SETPOINT windows; the payload value is the commanded net site
/// setpoint in kW.
pub fn parse_dispatch_windows(events: &[OadrEvent]) -> Vec<DispatchWindow> {
    timed_payloads(events, &["DISPATCH_SETPOINT"])
        .filter_map(|(event, timed, payload)| {
            Some(DispatchWindow {
                setpoint_kw: payload.numeric()?,
                start: timed.start,
                end: timed.end,
                event_id: event.id.to_string(),
            })
        })
        .collect()
}

/// The first CHARGE_STATE_SETPOINT (WP3.4): `(target_soc 0.0–1.0, window_end,
/// event_id)`. Values > 1 are read as percent (80 → 0.8); out-of-range
/// results are dropped.
pub fn parse_charge_state_setpoint(events: &[OadrEvent]) -> Option<(f64, DateTime<Utc>, String)> {
    timed_payloads(events, &["CHARGE_STATE_SETPOINT"]).find_map(|(event, timed, payload)| {
        let raw = payload.numeric()?;
        let target_soc = if raw > 1.0 { raw / 100.0 } else { raw };
        (0.0..=1.0)
            .contains(&target_soc)
            .then(|| (target_soc, timed.end, event.id.to_string()))
    })
}

// ---------------------------------------------------------------------------
// Report obligation extraction
// ---------------------------------------------------------------------------

/// When no interval of the event gives us a span to count in.
const DEFAULT_REPORT_CADENCE_S: u64 = 3600;

/// The report interval width this VEN chooses when the VTN leaves it open.
///
/// `reportIntervals: OPEN_INTERVALS` means "the VEN is expected to generate
/// intervals independent of the event's intervals" (User Guide 745). That is
/// what a standing monitoring event needs: it runs for a year, so its own grid
/// cannot be the report grid. The spec leaves the choice to the VEN, so the
/// lab writes its choice down -- `docs/reference/WIRE_PROFILE.md` -- rather
/// than leaving it implicit.
pub const OPEN_INTERVAL_WIDTH_S: u64 = 60;

/// How long one report interval covers, and how often to submit.
///
/// Two numbers, not one. 3.1's `frequency` is the "number of intervals that
/// elapse between reports", so a 60 s grid with `frequency: 4` means four 60 s
/// intervals in one submission every 240 s. Reading it as a single number
/// reported one 240 s bucket instead: coarser than asked for, at the right
/// moment.
///
/// The width comes from the event's own interval grid -- `event_timing`, the
/// one authority on when an interval runs -- unless the descriptor says
/// `OPEN_INTERVALS`, in which case it is the VEN's to choose.
fn report_timing(event: &OadrEvent, d: &WireReportDescriptor) -> (u64, u64) {
    use openleadr_wire::report::ReportIntervals;

    let intervals = timed_intervals(event);

    let width_s = if d.report_intervals == ReportIntervals::OpenIntervals {
        OPEN_INTERVAL_WIDTH_S
    } else if let Some(first) = intervals.first().filter(|i| i.is_bounded()) {
        (first.end - first.start).num_seconds().max(1) as u64
    } else {
        // A conformant peer may leave an event's timing open; say so rather
        // than pretend we derived a grid (`wire-contracts`: accept explicitly,
        // and surface the default you applied).
        debug!(
            event_id = %event.id,
            payload_type = %d.payload_type.wire_name(),
            default_s = DEFAULT_REPORT_CADENCE_S,
            "event declares no bounded interval and the descriptor does not              open the grid; applying the profile default report cadence"
        );
        return (DEFAULT_REPORT_CADENCE_S, DEFAULT_REPORT_CADENCE_S);
    };

    // Both fields are plain `i32` on the wire type with spec defaults of -1,
    // so "absent" and "-1" are the same branch rather than two.
    let n_intervals = if d.frequency > 0 {
        d.frequency as u64
    } else if d.num_intervals > 0 {
        d.num_intervals as u64
    } else if d.report_intervals == ReportIntervals::OpenIntervals {
        // An open grid has no event interval count to fall back on.
        1
    } else {
        intervals.len().max(1) as u64
    };

    (width_s, width_s.saturating_mul(n_intervals))
}

/// Extract report obligations from event reportDescriptors.
/// Deduplicates by (event_id, payload_type).
pub fn extract_report_obligations(
    events: &[OadrEvent],
    now: DateTime<Utc>,
    existing: &[OadrReportObligation],
) -> Vec<OadrReportObligation> {
    let mut result: Vec<OadrReportObligation> = Vec::new();

    for event in events {
        let event_id = event.id.to_string();
        let program_id = Some(event.content.program_id.to_string());

        let descriptors = match event.content.report_descriptors.as_ref() {
            Some(arr) if !arr.is_empty() => arr,
            _ => continue,
        };

        for descriptor in descriptors {
            let payload_type = descriptor.payload_type.wire_name();

            // Skip if already tracked
            let already_exists = existing
                .iter()
                .any(|ob| ob.event_id == event_id && ob.payload_type == payload_type)
                || result
                    .iter()
                    .any(|ob| ob.event_id == event_id && ob.payload_type == payload_type);

            if already_exists {
                continue;
            }

            let reading_type = descriptor
                .reading_type
                .as_ref()
                .map(|r| r.wire_name())
                .unwrap_or_else(|| "DIRECT_READ".to_string());

            let (interval_width_s, submit_every_s) = report_timing(event, descriptor);

            let due_at = now + Duration::seconds(submit_every_s as i64);

            result.push(OadrReportObligation {
                id: Uuid::new_v4(),
                event_id: event_id.clone(),
                program_id: program_id.clone(),
                payload_type,
                reading_type,
                resource_name: None,
                due_at,
                interval_width_s,
                submit_every_s,
                fulfilled: false,
                created_at: now,
                historical: descriptor.historical,
            });
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::vtn_port::OadrEvent;
    use chrono::TimeZone;
    use serde_json::json;

    // ── parse_alert_windows (WP3.1, BL-04) ──────────────────────────────────

    #[test]
    fn test_parse_alert_windows_event_level_period_fallback() {
        // Shape of User Guide Example 8.1-1: intervalPeriod at event level,
        // interval itself has only the payload.
        let events = json!([{
            "id": "alert-1",
            "programID": "prog-1",
            "eventName": "alertEvent",
            "intervalPeriod": { "start": "2026-03-14T00:00:00Z", "duration": "PT4H" },
            "intervals": [{
                "id": 0,
                "payloads": [{
                    "type": "ALERT_GRID_EMERGENCY",
                    "values": ["The grid is currently under emergency conditions"]
                }]
            }]
        }]);
        let alerts = parse_alert_windows(&crate::controller::vtn_port::events_from_json(events));
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].alert_type, "ALERT_GRID_EMERGENCY");
        assert_eq!(alerts[0].event_id, "alert-1");
        assert_eq!(alerts[0].start.to_rfc3339(), "2026-03-14T00:00:00+00:00");
        assert_eq!((alerts[0].end - alerts[0].start).num_hours(), 4);
        assert!(alerts[0].message.contains("emergency"));
    }

    #[test]
    fn test_parse_alert_windows_interval_level_period_wins() {
        let events = json!([{
            "id": "alert-2",
            "programID": "prog-1",
            "intervalPeriod": { "start": "2026-03-14T00:00:00Z", "duration": "PT4H" },
            "intervals": [{
                "id": 0,
                "intervalPeriod": { "start": "2026-03-14T02:00:00Z", "duration": "PT30M" },
                "payloads": [{ "type": "ALERT_BLACK_START", "values": ["restoring"] }]
            }]
        }]);
        let alerts = parse_alert_windows(&crate::controller::vtn_port::events_from_json(events));
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].alert_type, "ALERT_BLACK_START");
        assert_eq!(alerts[0].start.to_rfc3339(), "2026-03-14T02:00:00+00:00");
        assert_eq!((alerts[0].end - alerts[0].start).num_minutes(), 30);
    }

    #[test]
    fn test_parse_alert_windows_ignores_non_alert_events() {
        let events = json!([{
            "id": "evt-1",
            "programID": "prog-1",
            "intervals": [{
                "id": 0,
                "intervalPeriod": { "start": "2026-03-14T00:00:00Z", "duration": "PT1H" },
                "payloads": [{ "type": "PRICE", "values": [0.25] }]
            }]
        }]);
        let alerts = parse_alert_windows(&crate::controller::vtn_port::events_from_json(events));
        assert!(alerts.is_empty());
    }

    // ── parse_dispatch_windows / parse_charge_state_setpoint (WP3.4) ───────

    #[test]
    fn test_parse_dispatch_windows_extracts_setpoint_and_window() {
        let events = json!([{
            "id": "disp-1",
            "programID": "prog-1",
            "intervalPeriod": { "start": "2026-03-14T00:00:00Z", "duration": "PT15M" },
            "intervals": [{ "id": 0, "payloads": [{ "type": "DISPATCH_SETPOINT", "values": [1.5] }] }]
        }]);
        let w = parse_dispatch_windows(&crate::controller::vtn_port::events_from_json(events));
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].setpoint_kw, 1.5);
        assert_eq!((w[0].end - w[0].start).num_minutes(), 15);
        assert_eq!(w[0].event_id, "disp-1");
    }

    #[test]
    fn test_parse_charge_state_setpoint_fraction_and_percent() {
        let make = |val: serde_json::Value| {
            json!([{
                "id": "cs-1",
                "programID": "prog-1",
                "intervalPeriod": { "start": "2026-03-14T00:00:00Z", "duration": "PT2H" },
                "intervals": [{ "id": 0, "payloads": [{ "type": "CHARGE_STATE_SETPOINT", "values": [val] }] }]
            }])
        };
        let parse = |v| {
            parse_charge_state_setpoint(&crate::controller::vtn_port::events_from_json(make(v)))
        };
        let (soc, end, eid) = parse(json!(0.9)).expect("fraction accepted");
        assert!((soc - 0.9).abs() < 1e-9);
        assert_eq!(eid, "cs-1");
        assert_eq!(end.to_rfc3339(), "2026-03-14T02:00:00+00:00");

        let (soc, _, _) = parse(json!(85)).expect("percent accepted");
        assert!((soc - 0.85).abs() < 1e-9);

        assert!(parse(json!("full")).is_none(), "non-numeric dropped");
    }

    // ── parse_capacity_state export subscription/reservation (WP3.3) ───────

    #[test]
    fn test_parse_capacity_state_export_subscription_and_reservation() {
        let events = json!([{
            "id": "cap-1",
            "programID": "prog-1",
            "intervals": [{
                "id": 0,
                "intervalPeriod": { "start": "2026-03-14T00:00:00Z", "duration": "PT1H" },
                "payloads": [
                    { "type": "EXPORT_CAPACITY_SUBSCRIPTION", "values": [4.0] },
                    { "type": "EXPORT_CAPACITY_RESERVATION", "values": [2.0] }
                ]
            }]
        }]);
        let cap = parse_capacity_state(
            &crate::controller::vtn_port::events_from_json(events),
            Utc::now(),
        );
        assert_eq!(cap.export_subscription_kw, Some(4.0));
        assert_eq!(cap.export_reservation_kw, Some(2.0));
    }

    // ── parse_simple_windows (WP3.2) ────────────────────────────────────────

    #[test]
    fn test_parse_simple_windows_extracts_levels_and_window() {
        let events = json!([{
            "id": "simple-1",
            "programID": "prog-1",
            "intervalPeriod": { "start": "2026-03-14T00:00:00Z", "duration": "PT30M" },
            "intervals": [{ "id": 0, "payloads": [{ "type": "SIMPLE", "values": [2] }] }]
        }]);
        let windows = parse_simple_windows(&crate::controller::vtn_port::events_from_json(events));
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].level, 2);
        assert_eq!(windows[0].event_id, "simple-1");
        assert_eq!((windows[0].end - windows[0].start).num_minutes(), 30);
    }

    #[test]
    fn test_parse_simple_windows_drops_level_zero_and_out_of_range() {
        let events = json!([{
            "id": "simple-2",
            "programID": "prog-1",
            "intervalPeriod": { "start": "2026-03-14T00:00:00Z", "duration": "PT1H" },
            "intervals": [
                { "id": 0, "payloads": [{ "type": "SIMPLE", "values": [0] }] },
                { "id": 1, "payloads": [{ "type": "SIMPLE", "values": [7] }] },
                { "id": 2, "payloads": [{ "type": "SIMPLE", "values": ["high"] }] }
            ]
        }]);
        let windows = parse_simple_windows(&crate::controller::vtn_port::events_from_json(events));
        assert!(windows.is_empty());
    }

    #[test]
    fn test_parse_simple_windows_ignores_non_simple_payloads() {
        let events = json!([{
            "id": "evt-1",
            "programID": "prog-1",
            "intervals": [{
                "id": 0,
                "intervalPeriod": { "start": "2026-03-14T00:00:00Z", "duration": "PT1H" },
                "payloads": [{ "type": "PRICE", "values": [0.25] }]
            }]
        }]);
        let windows = parse_simple_windows(&crate::controller::vtn_port::events_from_json(events));
        assert!(windows.is_empty());
    }

    #[test]
    fn test_parse_alert_windows_untimed_alert_is_in_force_while_listed() {
        // No intervalPeriod anywhere: one project rule for every event type
        // (`controller::event_timing`, decided 2026-09-15) — in force for as long
        // as the VTN lists the event. Formerly skipped here while a timing-less
        // capacity limit applied, two rules for the same gap.
        let events = json!([{
            "id": "alert-3",
            "programID": "prog-1",
            "intervals": [{
                "id": 0,
                "payloads": [{ "type": "ALERT_GRID_EMERGENCY", "values": ["no window"] }]
            }]
        }]);
        let alerts = parse_alert_windows(&crate::controller::vtn_port::events_from_json(events));
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].start, crate::controller::event_timing::OPEN_START);
        assert_eq!(alerts[0].end, crate::controller::event_timing::OPEN_END);
    }

    // GB-48: intervals without their own period follow each other (User Guide
    // §7.3) — they no longer all inherit the whole event window.
    #[test]
    fn test_window_parsers_give_contiguous_intervals_their_own_windows() {
        let events = |payload_type: &str, v0: serde_json::Value, v1: serde_json::Value| {
            crate::controller::vtn_port::events_from_json(json!([{
                "id": "multi",
                "programID": "prog-1",
                "intervalPeriod": { "start": "2026-03-14T00:00:00Z", "duration": "PT30M" },
                "intervals": [
                    { "id": 0, "payloads": [{ "type": payload_type, "values": [v0] }] },
                    { "id": 1, "payloads": [{ "type": payload_type, "values": [v1] }] }
                ]
            }]))
        };
        let t = |s: &str| s.parse::<DateTime<Utc>>().unwrap();
        let (t0, t30, t60) = (
            t("2026-03-14T00:00:00Z"),
            t("2026-03-14T00:30:00Z"),
            t("2026-03-14T01:00:00Z"),
        );

        let alerts = parse_alert_windows(&events("ALERT_GRID_EMERGENCY", json!("a"), json!("b")));
        assert_eq!((alerts[0].start, alerts[0].end), (t0, t30));
        assert_eq!((alerts[1].start, alerts[1].end), (t30, t60));

        let simple = parse_simple_windows(&events("SIMPLE", json!(1), json!(3)));
        assert_eq!(
            (simple[1].level, simple[1].start, simple[1].end),
            (3, t30, t60)
        );

        let dispatch = parse_dispatch_windows(&events("DISPATCH_SETPOINT", json!(1.0), json!(2.0)));
        assert_eq!((dispatch[1].setpoint_kw, dispatch[1].start), (2.0, t30));

        let (_, end, _) =
            parse_charge_state_setpoint(&events("CHARGE_STATE_SETPOINT", json!(0.8), json!(0.9)))
                .unwrap();
        assert_eq!(end, t30, "the first interval's own end, not the event's");
    }

    #[test]
    fn test_parse_rate_snapshots_price() {
        let events = json!([
            {
                "id": "evt-1",
                "programID": "prog-1",
                "eventName": "price-event",
                "intervals": [
                    {
                        "id": 0,
                        "intervalPeriod": {
                            "start": "2025-01-01T14:00:00Z",
                            "duration": "PT1H"
                        },
                        "payloads": [
                            {"type": "PRICE", "values": [0.25]}
                        ]
                    },
                    {
                        "id": 1,
                        "intervalPeriod": {
                            "start": "2025-01-01T15:00:00Z",
                            "duration": "PT1H"
                        },
                        "payloads": [
                            {"type": "PRICE", "values": [0.30]}
                        ]
                    },
                    {
                        "id": 2,
                        "intervalPeriod": {
                            "start": "2025-01-01T16:00:00Z",
                            "duration": "PT1H"
                        },
                        "payloads": [
                            {"type": "PRICE", "values": [0.35]}
                        ]
                    }
                ]
            }
        ]);
        let snapshots =
            parse_rate_snapshots(&crate::controller::vtn_port::events_from_json(events));
        assert_eq!(snapshots.len(), 3);
        assert_eq!(snapshots[0].import_tariff_eur_kwh, Some(0.25));
        assert_eq!(snapshots[1].import_tariff_eur_kwh, Some(0.30));
        assert_eq!(snapshots[2].import_tariff_eur_kwh, Some(0.35));
    }

    #[test]
    fn test_parse_rate_snapshots_ghg() {
        let events = json!([
            {
                "id": "evt-ghg",
                "programID": "prog-1",
                "intervals": [
                    {
                        "id": 0,
                        "intervalPeriod": {
                            "start": "2025-01-01T10:00:00Z",
                            "duration": "PT1H"
                        },
                        "payloads": [
                            {"type": "GHG", "values": [200.0]}
                        ]
                    }
                ]
            }
        ]);
        let snapshots =
            parse_rate_snapshots(&crate::controller::vtn_port::events_from_json(events));
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].co2_g_kwh, Some(200.0));
    }

    /// BL-17 closeout: GHG must parse as a genuine multi-interval forecast, not
    /// just a single current value — mirrors `test_parse_rate_snapshots_price`
    /// exactly, since `collect_interval_groups` treats GHG identically to
    /// PRICE/EXPORT_PRICE (no GHG-specific code path exists).
    #[test]
    fn test_parse_rate_snapshots_ghg_multi_interval() {
        let events = json!([
            {
                "id": "evt-ghg-forecast",
                "programID": "prog-1",
                "eventName": "ghg-forecast-event",
                "intervals": [
                    {
                        "id": 0,
                        "intervalPeriod": {
                            "start": "2025-01-01T14:00:00Z",
                            "duration": "PT1H"
                        },
                        "payloads": [
                            {"type": "GHG", "values": [280.0]}
                        ]
                    },
                    {
                        "id": 1,
                        "intervalPeriod": {
                            "start": "2025-01-01T15:00:00Z",
                            "duration": "PT1H"
                        },
                        "payloads": [
                            {"type": "GHG", "values": [320.0]}
                        ]
                    },
                    {
                        "id": 2,
                        "intervalPeriod": {
                            "start": "2025-01-01T16:00:00Z",
                            "duration": "PT1H"
                        },
                        "payloads": [
                            {"type": "GHG", "values": [180.0]}
                        ]
                    }
                ]
            }
        ]);
        let snapshots =
            parse_rate_snapshots(&crate::controller::vtn_port::events_from_json(events));
        assert_eq!(snapshots.len(), 3);
        assert_eq!(snapshots[0].co2_g_kwh, Some(280.0));
        assert_eq!(snapshots[1].co2_g_kwh, Some(320.0));
        assert_eq!(snapshots[2].co2_g_kwh, Some(180.0));
    }

    #[test]
    fn test_parse_rate_snapshots_export_price() {
        let events = json!([
            {
                "id": "evt-export",
                "programID": "prog-1",
                "intervals": [
                    {
                        "id": 0,
                        "intervalPeriod": {
                            "start": "2025-01-01T12:00:00Z",
                            "duration": "PT1H"
                        },
                        "payloads": [
                            {"type": "EXPORT_PRICE", "values": [0.10]}
                        ]
                    }
                ]
            }
        ]);
        let snapshots =
            parse_rate_snapshots(&crate::controller::vtn_port::events_from_json(events));
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].export_tariff_eur_kwh, Some(0.10));
    }

    #[test]
    fn test_parse_capacity_schedule_keeps_per_interval_limits() {
        let events = json!([
            {
                "id": "evt-cap-sched",
                "programID": "prog-1",
                "intervals": [
                    {
                        "id": 0,
                        "intervalPeriod": {
                            "start": "2025-01-01T10:00:00Z",
                            "duration": "PT1H"
                        },
                        "payloads": [
                            {"type": "IMPORT_CAPACITY_LIMIT", "values": [5.0]}
                        ]
                    },
                    {
                        "id": 1,
                        "intervalPeriod": {
                            "start": "2025-01-01T11:00:00Z",
                            "duration": "PT1H"
                        },
                        "payloads": [
                            {"type": "IMPORT_CAPACITY_LIMIT", "values": [3.0]},
                            {"type": "EXPORT_CAPACITY_LIMIT", "values": [2.0]}
                        ]
                    }
                ]
            }
        ]);
        let snapshots =
            parse_capacity_schedule(&crate::controller::vtn_port::events_from_json(events));
        // Unlike parse_capacity_state (which collapses to the strictest single value),
        // the schedule keeps both intervals with their own distinct limits.
        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].import_limit_kw, Some(5.0));
        assert_eq!(snapshots[0].export_limit_kw, None);
        assert_eq!(snapshots[1].import_limit_kw, Some(3.0));
        assert_eq!(snapshots[1].export_limit_kw, Some(2.0));
    }

    #[test]
    fn test_parse_capacity_schedule_ignores_non_capacity_payloads() {
        let events = json!([
            {
                "id": "evt-price-only",
                "programID": "prog-1",
                "intervals": [
                    {
                        "id": 0,
                        "intervalPeriod": {
                            "start": "2025-01-01T10:00:00Z",
                            "duration": "PT1H"
                        },
                        "payloads": [
                            {"type": "PRICE", "values": [0.25]}
                        ]
                    }
                ]
            }
        ]);
        let snapshots =
            parse_capacity_schedule(&crate::controller::vtn_port::events_from_json(events));
        assert!(snapshots.is_empty());
    }

    // GB-33: `experiments/run_experiment.py`'s `build_event()` (and, per the
    // OpenADR 3 spec, any single-window capacity event) sets `intervalPeriod`
    // only at the EVENT level, not on the lone interval itself -- unlike the
    // fixtures above (and every PRICE event this project's own tooling
    // sends), which always give each interval its own `intervalPeriod`. That
    // gap meant `grid_samples.import_limit_kw`/`export_limit_kw` had never
    // once been populated on any VEN, across the whole deployment's history
    // (confirmed empirically, see docs/BACKLOG.md GB-33) -- the real
    // functional enforcement (`parse_capacity_state`, feeding the planner)
    // was unaffected since it never required per-interval timing, but the
    // schedule this test covers silently dropped every such event.
    #[test]
    fn test_parse_capacity_schedule_falls_back_to_event_level_interval_period() {
        let events = json!([
            {
                "id": "evt-cap-single",
                "programID": "prog-1",
                "intervalPeriod": {
                    "start": "2025-01-01T10:00:00Z",
                    "duration": "PT20M"
                },
                "intervals": [
                    {
                        "id": 0,
                        "payloads": [
                            {"type": "IMPORT_CAPACITY_LIMIT", "values": [1.5]}
                        ]
                    }
                ]
            }
        ]);
        let snapshots =
            parse_capacity_schedule(&crate::controller::vtn_port::events_from_json(events));
        assert_eq!(
            snapshots.len(),
            1,
            "event-level intervalPeriod must be used as a fallback"
        );
        assert_eq!(snapshots[0].import_limit_kw, Some(1.5));
        assert_eq!(
            snapshots[0].interval_start,
            "2025-01-01T10:00:00Z".parse::<DateTime<Utc>>().unwrap()
        );
        assert_eq!(
            snapshots[0].interval_end,
            "2025-01-01T10:20:00Z".parse::<DateTime<Utc>>().unwrap()
        );
    }

    // GB-48: a multi-interval event without per-interval `intervalPeriod`s is
    // not ambiguous — User Guide §7.3 makes the intervals contiguous from the
    // event-level start, each lasting the event-level duration, and Example
    // 8.10.1-1 (Dynamic Operating Envelope) uses exactly this shape. This test
    // used to assert an empty schedule ("does not guess"); that dropped every
    // spec-form DOE.
    #[test]
    fn test_parse_capacity_schedule_multi_interval_events_are_contiguous() {
        let events = json!([
            {
                "id": "evt-cap-multi",
                "programID": "prog-1",
                "intervalPeriod": {
                    "start": "2025-01-01T10:00:00Z",
                    "duration": "PT2H"
                },
                "intervals": [
                    {"id": 0, "payloads": [{"type": "IMPORT_CAPACITY_LIMIT", "values": [5.0]}]},
                    {"id": 1, "payloads": [{"type": "IMPORT_CAPACITY_LIMIT", "values": [3.0]}]}
                ]
            }
        ]);
        let snapshots =
            parse_capacity_schedule(&crate::controller::vtn_port::events_from_json(events));
        let t = |s: &str| s.parse::<DateTime<Utc>>().unwrap();
        let got: Vec<_> = snapshots
            .iter()
            .map(|c| (c.interval_start, c.interval_end, c.import_limit_kw))
            .collect();
        assert_eq!(
            got,
            vec![
                (
                    t("2025-01-01T10:00:00Z"),
                    t("2025-01-01T12:00:00Z"),
                    Some(5.0)
                ),
                (
                    t("2025-01-01T12:00:00Z"),
                    t("2025-01-01T14:00:00Z"),
                    Some(3.0)
                ),
            ]
        );
        assert_eq!(
            snapshots[0].import_limit_event_id.as_deref(),
            Some("evt-cap-multi")
        );
    }

    #[test]
    fn test_parse_capacity_state_import_limit() {
        let events = json!([
            {
                "id": "evt-cap",
                "programID": "prog-1",
                "intervals": [
                    {
                        "id": 0,
                        "intervalPeriod": {
                            "start": "2025-01-01T10:00:00Z",
                            "duration": "PT1H"
                        },
                        "payloads": [
                            {"type": "IMPORT_CAPACITY_LIMIT", "values": [5.0]}
                        ]
                    }
                ]
            }
        ]);
        let events = crate::controller::vtn_port::events_from_json(events);
        // GB-48: the limit fields mean "in force now". At 09:00 the 10:00–11:00
        // limit has not started — this test used to expect 5.0 here, which is
        // exactly the bug (a future limit applied now).
        let before = Utc.with_ymd_and_hms(2025, 1, 1, 9, 0, 0).unwrap();
        let cap = parse_capacity_state(&events, before);
        assert_eq!(
            cap.import_limit_kw, None,
            "not in force before its interval"
        );
        assert_eq!(
            cap.last_updated,
            Some(before),
            "last_updated must equal the injected clock, not wall-clock Utc::now()"
        );
        let during = Utc.with_ymd_and_hms(2025, 1, 1, 10, 30, 0).unwrap();
        let cap = parse_capacity_state(&events, during);
        assert_eq!(cap.import_limit_kw, Some(5.0));
        assert_eq!(cap.import_limit_event_id, Some("evt-cap".to_string()));
        let after = Utc.with_ymd_and_hms(2025, 1, 1, 11, 0, 0).unwrap();
        assert_eq!(parse_capacity_state(&events, after).import_limit_kw, None);
    }

    // GB-48: overlapping limits resolve like every other payload type — by
    // event priority, then creation time (GB-45, User Guide §7.1) — not
    // "strictest wins", which was a second rule contradicting the schedule the
    // history records. Formerly `test_parse_capacity_state_strictest_wins`.
    #[test]
    fn test_parse_capacity_state_overlapping_limits_resolve_by_priority() {
        let events = json!([
            {
                "id": "evt-a",
                "programID": "prog-1",
                "priority": 1,
                "intervals": [{
                    "id": 0,
                    "intervalPeriod": {"start": "2025-01-01T10:00:00Z", "duration": "PT1H"},
                    "payloads": [{"type": "IMPORT_CAPACITY_LIMIT", "values": [10.0]}]
                }]
            },
            {
                "id": "evt-b",
                "programID": "prog-1",
                "priority": 5,
                "intervals": [{
                    "id": 0,
                    "intervalPeriod": {"start": "2025-01-01T10:00:00Z", "duration": "PT1H"},
                    "payloads": [{"type": "IMPORT_CAPACITY_LIMIT", "values": [3.0]}]
                }]
            }
        ]);
        let now = Utc.with_ymd_and_hms(2025, 1, 1, 10, 30, 0).unwrap();
        let cap = parse_capacity_state(&crate::controller::vtn_port::events_from_json(events), now);
        assert_eq!(
            cap.import_limit_kw,
            Some(10.0),
            "priority 1 wins over priority 5"
        );
        assert_eq!(cap.import_limit_event_id, Some("evt-a".to_string()));
    }

    #[test]
    fn test_parse_capacity_state_untimed_limit_is_in_force_while_listed() {
        // The E2E use-case events send limits without any intervalPeriod.
        let events = json!([{
            "id": "evt-untimed",
            "programID": "prog-1",
            "intervals": [{"id": 0, "payloads": [{"type": "IMPORT_CAPACITY_LIMIT", "values": [10000.0]}]}]
        }]);
        let cap = parse_capacity_state(
            &crate::controller::vtn_port::events_from_json(events),
            Utc::now(),
        );
        assert_eq!(cap.import_limit_kw, Some(10000.0));
    }

    #[test]
    fn test_extract_report_obligations_empty_when_no_descriptors() {
        let events = json!([
            {
                "id": "evt-1",
                "programID": "prog-1",
                "intervals": []
            }
        ]);
        let now = Utc::now();
        let obligations = extract_report_obligations(
            &crate::controller::vtn_port::events_from_json(events),
            now,
            &[],
        );
        assert!(obligations.is_empty());
    }

    #[test]
    fn test_extract_report_obligations_with_descriptor() {
        let events = json!([
            {
                "id": "evt-1",
                "programID": "prog-1",
                "reportDescriptors": [
                    {
                        "payloadType": "USAGE",
                        "readingType": "DIRECT_READ"
                    }
                ],
                "intervals": []
            }
        ]);
        let now = Utc::now();
        let obligations = extract_report_obligations(
            &crate::controller::vtn_port::events_from_json(events),
            now,
            &[],
        );
        assert_eq!(obligations.len(), 1);
        assert_eq!(obligations[0].payload_type, "USAGE");
        assert_eq!(obligations[0].reading_type, "DIRECT_READ");
        assert!(!obligations[0].fulfilled);
        assert!(
            obligations[0].historical,
            "absent reportDescriptor.historical defaults to true per spec"
        );
    }

    #[test]
    fn extract_report_obligations_parses_historical_false() {
        // R-15: `historical: false` marks the obligation as a forecast request.
        let events = json!([{
            "id": "evt-h",
            "programID": "prog-1",
            "reportDescriptors": [
                {"payloadType": "USAGE", "frequency": 900, "historical": false}
            ]
        }]);
        let obligations = extract_report_obligations(
            &crate::controller::vtn_port::events_from_json(events),
            Utc::now(),
            &[],
        );
        assert_eq!(obligations.len(), 1);
        assert!(!obligations[0].historical);
    }

    #[test]
    fn test_parse_rate_snapshots_no_loop_when_duration_equals_cycle() {
        // event.intervalPeriod.duration == sum of intervals → no looping
        let events = json!([{
            "id": "evt-noloop",
            "programID": "prog-1",
            // event.duration exactly equal to the interval span: the sequence
            // reaches the window's end, so there is nothing to repeat.
            "duration": "PT2H",
            "intervalPeriod": {
                "start": "2026-03-17T00:00:00Z"
            },
            "intervals": [
                {
                    "id": 0,
                    "intervalPeriod": {"start": "2026-03-17T00:00:00Z", "duration": "PT1H"},
                    "payloads": [{"type": "PRICE", "values": [0.10]}]
                },
                {
                    "id": 1,
                    "intervalPeriod": {"start": "2026-03-17T01:00:00Z", "duration": "PT1H"},
                    "payloads": [{"type": "PRICE", "values": [0.20]}]
                }
            ]
        }]);
        let snapshots =
            parse_rate_snapshots(&crate::controller::vtn_port::events_from_json(events));
        assert_eq!(
            snapshots.len(),
            2,
            "no looping expected when duration == cycle"
        );
    }

    #[test]
    fn test_parse_rate_snapshots_looping_covers_now() {
        // 2-hour cycle starting 2026-01-01, P9999Y duration → should loop
        // now is 2 days later — original intervals are long expired
        let now: DateTime<Utc> = "2026-01-03T00:30:00Z".parse().unwrap();
        let events = json!([{
            "id": "evt-loop",
            "programID": "prog-1",
            "duration": "P9999Y",
            "intervalPeriod": {"start": "2026-01-01T00:00:00Z"},
            "intervals": [
                {
                    "id": 0,
                    "intervalPeriod": {"start": "2026-01-01T00:00:00Z", "duration": "PT1H"},
                    "payloads": [{"type": "PRICE", "values": [0.10]}]
                },
                {
                    "id": 1,
                    "intervalPeriod": {"start": "2026-01-01T01:00:00Z", "duration": "PT1H"},
                    "payloads": [{"type": "PRICE", "values": [0.20]}]
                }
            ]
        }]);
        let snapshots =
            parse_rate_snapshots(&crate::controller::vtn_port::events_from_json(events));

        // More than 2 intervals: looping occurred
        assert!(
            snapshots.len() > 2,
            "expected looped intervals, got {}",
            snapshots.len()
        );

        // An interval must cover now (2026-01-03T00:30 → cycle 24, interval 0: 00:00–01:00)
        let current = crate::entities::tariff_snapshot::tariff_at(&snapshots, now);
        assert!(current.is_some(), "no interval covers now");
        assert_eq!(current.unwrap().import_tariff_eur_kwh, Some(0.10));
    }

    #[test]
    fn test_parse_rate_snapshots_looping_has_future_intervals() {
        let now: DateTime<Utc> = "2026-01-03T00:30:00Z".parse().unwrap();
        let events = json!([{
            "id": "evt-loop",
            "programID": "prog-1",
            "duration": "P9999Y",
            "intervalPeriod": {"start": "2026-01-01T00:00:00Z"},
            "intervals": [
                {
                    "id": 0,
                    "intervalPeriod": {"start": "2026-01-01T00:00:00Z", "duration": "PT1H"},
                    "payloads": [{"type": "PRICE", "values": [0.10]}]
                },
                {
                    "id": 1,
                    "intervalPeriod": {"start": "2026-01-01T01:00:00Z", "duration": "PT1H"},
                    "payloads": [{"type": "PRICE", "values": [0.20]}]
                }
            ]
        }]);
        let snapshots =
            parse_rate_snapshots(&crate::controller::vtn_port::events_from_json(events));
        assert!(
            snapshots.iter().any(|s| s.interval_start > now),
            "expected at least one future interval"
        );
    }

    #[test]
    fn test_parse_rate_snapshots_looping_24h_cycle() {
        // 24 hourly intervals (like the seed price event), P9999Y → daily repeat
        // now is 2 days + 14.5 h after base midnight
        let now: DateTime<Utc> = "2026-01-03T14:30:00Z".parse().unwrap();

        let intervals: Vec<serde_json::Value> = (0u32..24)
            .map(|h| {
                json!({
                    "id": h,
                    "intervalPeriod": {
                        "start": format!("2026-01-01T{:02}:00:00Z", h),
                        "duration": "PT1H"
                    },
                    "payloads": [{"type": "PRICE", "values": [h as f64]}]
                })
            })
            .collect();

        let events = json!([{
            "id": "evt-daily",
            "programID": "prog-1",
            "duration": "P9999Y",
            "intervalPeriod": {"start": "2026-01-01T00:00:00Z"},
            "intervals": intervals
        }]);
        let snapshots =
            parse_rate_snapshots(&crate::controller::vtn_port::events_from_json(events));

        assert!(
            snapshots.len() > 24,
            "expected more than 24 intervals (looping), got {}",
            snapshots.len()
        );

        // now = 2026-01-03T14:30 → cycle 2 (offset 2×86400s), hour 14 → price = 14.0
        let current = crate::entities::tariff_snapshot::tariff_at(&snapshots, now);
        assert!(current.is_some(), "no interval covers now at {}", now);
        assert_eq!(current.unwrap().import_tariff_eur_kwh, Some(14.0));
    }

    // ── BL-02: priority-ordered merge ────────────────────────────────────

    fn price_event(
        id: &str,
        priority: Option<i64>,
        created: Option<&str>,
        price: f64,
    ) -> OadrEvent {
        crate::controller::vtn_port::events_from_json(json!({
            "id": id,
            "programID": "prog-1",
            "priority": priority,
            "createdDateTime": created,
            "intervals": [{
                "id": 0,
                "intervalPeriod": {"start": "2026-02-01T10:00:00Z", "duration": "PT1H"},
                "payloads": [{"type": "PRICE", "values": [price]}]
            }]
        }))
        .remove(0)
    }

    #[test]
    fn test_parse_rate_snapshots_higher_priority_wins_regardless_of_order() {
        let high = price_event("evt-high", Some(1), Some("2026-01-01T00:00:00Z"), 0.50);
        let low = price_event("evt-low", Some(5), Some("2026-01-01T00:00:00Z"), 0.10);

        for events in [
            vec![high.clone(), low.clone()],
            vec![low.clone(), high.clone()],
        ] {
            let snapshots = parse_rate_snapshots(&events);
            assert_eq!(snapshots.len(), 1);
            assert_eq!(
                snapshots[0].import_tariff_eur_kwh,
                Some(0.50),
                "priority 1 must beat priority 5 regardless of array order"
            );
        }
    }

    #[test]
    fn test_parse_rate_snapshots_equal_priority_newer_created_wins() {
        let newer = price_event("evt-new", Some(2), Some("2026-02-01T08:00:00Z"), 0.40);
        let older = price_event("evt-old", Some(2), Some("2026-01-15T08:00:00Z"), 0.20);

        // older last in the array — would win under naive last-write-wins
        let snapshots = parse_rate_snapshots(&[newer, older]);
        assert_eq!(snapshots.len(), 1);
        assert_eq!(
            snapshots[0].import_tariff_eur_kwh,
            Some(0.40),
            "newer createdDateTime must win at equal priority"
        );
    }

    #[test]
    fn test_parse_rate_snapshots_absent_priority_sorts_last() {
        let explicit = price_event("evt-p5", Some(5), Some("2026-01-01T00:00:00Z"), 0.30);
        let none = price_event("evt-none", None, Some("2026-02-01T00:00:00Z"), 0.99);

        // None-priority event last in the array — would win under naive last-write-wins
        let snapshots = parse_rate_snapshots(&[explicit, none]);
        assert_eq!(snapshots.len(), 1);
        assert_eq!(
            snapshots[0].import_tariff_eur_kwh,
            Some(0.30),
            "an event with any explicit priority must beat one without"
        );
    }

    // ── GB-45: priority across partially overlapping intervals ───────────

    fn ts(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    /// Single-interval event over `[start, start + duration)` carrying `payloads`.
    fn window_event(
        id: &str,
        priority: Option<i64>,
        created: &str,
        start: &str,
        duration: &str,
        payloads: &[(&str, f64)],
    ) -> OadrEvent {
        let payloads: Vec<serde_json::Value> = payloads
            .iter()
            .map(|(t, v)| json!({"type": t, "values": [v]}))
            .collect();
        crate::controller::vtn_port::events_from_json(json!({
            "id": id,
            "programID": "prog-1",
            "priority": priority,
            "createdDateTime": created,
            "intervals": [{
                "id": 0,
                "intervalPeriod": {"start": start, "duration": duration},
                "payloads": payloads
            }]
        }))
        .remove(0)
    }

    fn import_at(
        snaps: &[crate::entities::tariff_snapshot::TariffSnapshot],
        at: &str,
    ) -> Option<f64> {
        let at = ts(at);
        crate::entities::tariff_snapshot::tariff_at(snaps, at).and_then(|s| s.import_tariff_eur_kwh)
    }

    fn segments(
        snaps: &[crate::entities::tariff_snapshot::TariffSnapshot],
    ) -> Vec<(DateTime<Utc>, DateTime<Utc>, Option<f64>)> {
        snaps
            .iter()
            .map(|s| (s.interval_start, s.interval_end, s.import_tariff_eur_kwh))
            .collect()
    }

    fn day_ahead_and_intra_hour() -> Vec<OadrEvent> {
        vec![
            window_event(
                "tou",
                Some(5),
                "2026-08-31T00:00:00Z",
                "2026-08-31T05:00:00Z",
                "PT1H",
                &[("PRICE", 0.09)],
            ),
            window_event(
                "dr",
                Some(1),
                "2026-08-30T00:00:00Z",
                "2026-08-31T05:20:00Z",
                "PT10M",
                &[("PRICE", 0.45)],
            ),
        ]
    }

    #[test]
    fn parse_rate_snapshots_intra_hour_high_priority_splits_hour() {
        for events in [
            day_ahead_and_intra_hour(),
            day_ahead_and_intra_hour().into_iter().rev().collect(),
        ] {
            let snaps = parse_rate_snapshots(&events);
            assert_eq!(
                segments(&snaps),
                vec![
                    (ts("2026-08-31T05:00:00Z"), ts("2026-08-31T05:20:00Z"), Some(0.09)),
                    (ts("2026-08-31T05:20:00Z"), ts("2026-08-31T05:30:00Z"), Some(0.45)),
                    (ts("2026-08-31T05:30:00Z"), ts("2026-08-31T06:00:00Z"), Some(0.09)),
                ],
                "the 10-min priority-1 price must apply for exactly its window, the hour's own price around it"
            );
        }
    }

    #[test]
    fn parse_rate_snapshots_earlier_start_lower_priority_loses() {
        // S-2 of the 2026-08-31 fleet run: hourly TOU from 05:00, scenario window from 05:29.
        let events = vec![
            window_event(
                "tou",
                Some(5),
                "2026-08-31T00:00:00Z",
                "2026-08-31T05:00:00Z",
                "PT1H",
                &[("PRICE", 0.09)],
            ),
            window_event(
                "scn",
                Some(1),
                "2026-08-30T00:00:00Z",
                "2026-08-31T05:29:00Z",
                "PT30M",
                &[("PRICE", 0.10)],
            ),
        ];
        let snaps = parse_rate_snapshots(&events);
        assert_eq!(import_at(&snaps, "2026-08-31T05:28:59Z"), Some(0.09));
        for minute in 29..59 {
            assert_eq!(
                import_at(&snaps, &format!("2026-08-31T05:{minute:02}:00Z")),
                Some(0.10),
                "05:{minute:02} must resolve to the priority-1 event despite its later start"
            );
        }
        assert_eq!(import_at(&snaps, "2026-08-31T05:59:30Z"), Some(0.09));
    }

    #[test]
    fn parse_rate_snapshots_partial_overlap_equal_priority_newer_wins() {
        let older = window_event(
            "old",
            Some(2),
            "2026-01-15T08:00:00Z",
            "2026-02-01T10:00:00Z",
            "PT1H",
            &[("PRICE", 0.20)],
        );
        let newer = window_event(
            "new",
            Some(2),
            "2026-02-01T08:00:00Z",
            "2026-02-01T10:30:00Z",
            "PT1H",
            &[("PRICE", 0.40)],
        );
        for events in [
            vec![older.clone(), newer.clone()],
            vec![newer.clone(), older.clone()],
        ] {
            let snaps = parse_rate_snapshots(&events);
            assert_eq!(
                segments(&snaps),
                vec![
                    (
                        ts("2026-02-01T10:00:00Z"),
                        ts("2026-02-01T10:30:00Z"),
                        Some(0.20)
                    ),
                    (
                        ts("2026-02-01T10:30:00Z"),
                        ts("2026-02-01T11:00:00Z"),
                        Some(0.40)
                    ),
                    (
                        ts("2026-02-01T11:00:00Z"),
                        ts("2026-02-01T11:30:00Z"),
                        Some(0.40)
                    ),
                ]
            );
        }
    }

    #[test]
    fn parse_rate_snapshots_absent_priority_ranks_lowest_on_partial_overlap() {
        let none = window_event(
            "none",
            None,
            "2026-02-01T09:00:00Z",
            "2026-02-01T10:00:00Z",
            "PT1H",
            &[("PRICE", 0.99)],
        );
        let p9 = window_event(
            "p9",
            Some(9),
            "2026-01-01T00:00:00Z",
            "2026-02-01T10:30:00Z",
            "PT1H",
            &[("PRICE", 0.30)],
        );
        let snaps = parse_rate_snapshots(&[p9, none]);
        assert_eq!(import_at(&snaps, "2026-02-01T10:15:00Z"), Some(0.99));
        assert_eq!(import_at(&snaps, "2026-02-01T10:45:00Z"), Some(0.30));
        assert_eq!(import_at(&snaps, "2026-02-01T11:15:00Z"), Some(0.30));
    }

    #[test]
    fn parse_rate_snapshots_resolves_each_payload_type_independently() {
        let price_only = window_event(
            "p1",
            Some(1),
            "2026-01-01T00:00:00Z",
            "2026-02-01T10:15:00Z",
            "PT30M",
            &[("PRICE", 0.45)],
        );
        let price_ghg = window_event(
            "p5",
            Some(5),
            "2026-01-01T00:00:00Z",
            "2026-02-01T10:00:00Z",
            "PT1H",
            &[("PRICE", 0.09), ("GHG", 300.0)],
        );
        let snaps = parse_rate_snapshots(&[price_only, price_ghg]);
        let at = |s: &str| {
            let t = ts(s);
            let snap = crate::entities::tariff_snapshot::tariff_at(&snaps, t).expect("covered");
            (snap.import_tariff_eur_kwh, snap.co2_g_kwh)
        };
        assert_eq!(at("2026-02-01T10:05:00Z"), (Some(0.09), Some(300.0)));
        assert_eq!(at("2026-02-01T10:30:00Z"), (Some(0.45), Some(300.0)));
        assert_eq!(at("2026-02-01T10:50:00Z"), (Some(0.09), Some(300.0)));
    }

    #[test]
    fn parse_rate_snapshots_output_never_overlaps() {
        // Looping hourly day-ahead price (P9999Y) plus two short DR prices, one straddling
        // an hour boundary: the published schedule must be sorted and non-overlapping.
        let intervals: Vec<serde_json::Value> = (0u32..24)
            .map(|h| {
                json!({
                    "id": h,
                    "intervalPeriod": {"start": format!("2026-01-01T{:02}:00:00Z", h), "duration": "PT1H"},
                    "payloads": [{"type": "PRICE", "values": [h as f64]}]
                })
            })
            .collect();
        let mut events: Vec<OadrEvent> = crate::controller::vtn_port::events_from_json(json!([{
            "id": "evt-daily",
            "programID": "prog-1",
            "priority": 5,
            "duration": "P9999Y",
            "intervalPeriod": {"start": "2026-01-01T00:00:00Z"},
            "intervals": intervals
        }]));
        events.push(window_event(
            "dr1",
            Some(1),
            "2026-01-03T00:00:00Z",
            "2026-01-03T15:20:00Z",
            "PT10M",
            &[("PRICE", 99.0)],
        ));
        events.push(window_event(
            "dr2",
            Some(1),
            "2026-01-03T00:00:00Z",
            "2026-01-03T16:50:00Z",
            "PT20M",
            &[("PRICE", 77.0)],
        ));

        let snaps = parse_rate_snapshots(&events);
        for w in snaps.windows(2) {
            assert!(
                w[0].interval_end <= w[1].interval_start,
                "overlap: [{}, {}) and [{}, {})",
                w[0].interval_start,
                w[0].interval_end,
                w[1].interval_start,
                w[1].interval_end
            );
        }
        assert_eq!(import_at(&snaps, "2026-01-03T15:10:00Z"), Some(15.0));
        assert_eq!(import_at(&snaps, "2026-01-03T15:25:00Z"), Some(99.0));
        assert_eq!(import_at(&snaps, "2026-01-03T15:35:00Z"), Some(15.0));
        assert_eq!(import_at(&snaps, "2026-01-03T16:55:00Z"), Some(77.0));
        assert_eq!(import_at(&snaps, "2026-01-03T17:05:00Z"), Some(77.0));
        assert_eq!(import_at(&snaps, "2026-01-03T17:15:00Z"), Some(17.0));
    }

    #[test]
    fn parse_capacity_schedule_short_high_priority_limit_splits_longer_one() {
        let events = vec![
            window_event(
                "env",
                Some(5),
                "2026-09-09T00:00:00Z",
                "2026-09-09T18:00:00Z",
                "PT4H",
                &[("IMPORT_CAPACITY_LIMIT", 6.0)],
            ),
            window_event(
                "cap",
                Some(1),
                "2026-09-08T00:00:00Z",
                "2026-09-09T19:00:00Z",
                "PT30M",
                &[("IMPORT_CAPACITY_LIMIT", 2.0)],
            ),
        ];
        let got: Vec<_> = parse_capacity_schedule(&events)
            .into_iter()
            .map(|c| (c.interval_start, c.interval_end, c.import_limit_kw))
            .collect();
        assert_eq!(
            got,
            vec![
                (
                    ts("2026-09-09T18:00:00Z"),
                    ts("2026-09-09T19:00:00Z"),
                    Some(6.0)
                ),
                (
                    ts("2026-09-09T19:00:00Z"),
                    ts("2026-09-09T19:30:00Z"),
                    Some(2.0)
                ),
                (
                    ts("2026-09-09T19:30:00Z"),
                    ts("2026-09-09T22:00:00Z"),
                    Some(6.0)
                ),
            ]
        );
    }

    #[test]
    fn parse_rate_snapshots_planner_series_agrees_with_tick_lookup() {
        use crate::entities::tariff_snapshot::TariffTimeSeries;
        let snaps = parse_rate_snapshots(&day_ahead_and_intra_hour());
        let series = TariffTimeSeries::from_snapshots(&snaps);
        for at in [
            "2026-08-31T05:10:00Z",
            "2026-08-31T05:25:00Z",
            "2026-08-31T05:35:00Z",
            "2026-08-31T05:55:00Z",
        ] {
            assert_eq!(
                series.import_eur_kwh.interpolate_at(ts(at)),
                import_at(&snaps, at),
                "planner series and tick-time lookup disagree at {at}"
            );
        }
    }

    #[test]
    fn test_extract_report_obligations_dedup() {
        // Dedup by (event_id, payload_type) is intentionally unconditional — it does
        // not check `fulfilled`. Recurrence (R6) is handled by re-arming the *same*
        // obligation's `due_at` in place (`AppState::rearm_obligation`), not by letting
        // this function regenerate a fresh one each cycle; regenerating here would
        // orphan the original (and its VTN report name history) instead of advancing
        // it. See docs/plans/review_items_resolution_strategy.md R6.
        let events = json!([
            {
                "id": "evt-1",
                "programID": "prog-1",
                "reportDescriptors": [
                    {"payloadType": "USAGE", "readingType": "DIRECT_READ"}
                ],
                "intervals": []
            }
        ]);
        let now = Utc::now();
        // Simulate already having an obligation for this event+type
        let existing = vec![OadrReportObligation {
            id: Uuid::new_v4(),
            event_id: "evt-1".to_string(),
            program_id: Some("prog-1".to_string()),
            payload_type: "USAGE".to_string(),
            reading_type: "DIRECT_READ".to_string(),
            resource_name: None,
            due_at: now,
            interval_width_s: 3600,
            submit_every_s: 3600,
            fulfilled: false,
            created_at: now,
            historical: true,
        }];
        let obligations = extract_report_obligations(
            &crate::controller::vtn_port::events_from_json(events),
            now,
            &existing,
        );
        // Should not add a duplicate
        assert!(obligations.is_empty());
    }

    /// A report interval's width and the submission cadence are two different
    /// numbers, and conflating them gets both wrong.
    ///
    /// `frequency` counts intervals *between reports*: with a 60 s grid and
    /// `frequency: 4`, one submission every 240 s carrying four 60 s
    /// intervals. The obligation stored a single number for both, so it
    /// reported one 240 s bucket instead -- coarser data than asked for, at
    /// the right moment, which is the kind of wrong that looks right on a
    /// chart.
    #[test]
    fn report_width_and_cadence_are_separate_numbers() {
        let events = minutely_event(json!({
            "payloadType": "DEMAND", "frequency": 4
        }));
        let obligations = extract_report_obligations(&events, Utc::now(), &[]);
        let ob = &obligations[0];
        assert_eq!(ob.interval_width_s, 60, "one interval covers 60 s");
        assert_eq!(
            ob.submit_every_s, 240,
            "four of them elapse between reports"
        );
    }

    /// A standing monitoring event is one long interval -- a year -- so its
    /// grid cannot set the report grid. `reportIntervals: OPEN_INTERVALS` is
    /// the spec's way of saying "the VEN generates intervals independent of
    /// the event's" (User Guide 745), and the lab's choice is written down in
    /// `docs/reference/WIRE_PROFILE.md` rather than left implicit.
    #[test]
    fn open_intervals_lets_the_ven_choose_its_own_grid() {
        let events = crate::controller::vtn_port::events_from_json(json!([{
            "id": "evt-standing",
            "programID": "fleet-monitoring",
            "duration": "P1Y",
            "intervalPeriod": {"start": "2026-01-01T00:00:00Z", "duration": "P1Y"},
            "intervals": [{
                "id": 0,
                "payloads": [{"type": "SIMPLE", "values": [0]}]
            }],
            "reportDescriptors": [{
                "payloadType": "DEMAND",
                "readingType": "MEAN",
                "frequency": 1,
                "reportIntervals": "OPEN_INTERVALS"
            }]
        }]));
        let obligations = extract_report_obligations(&events, Utc::now(), &[]);
        let ob = &obligations[0];
        assert_eq!(
            ob.interval_width_s, OPEN_INTERVAL_WIDTH_S,
            "a year-long event must not become a year-long report interval"
        );
        assert_eq!(ob.submit_every_s, OPEN_INTERVAL_WIDTH_S);
    }

    /// Four 60-second intervals, for the width/cadence split above.
    fn minutely_event(descriptor: serde_json::Value) -> Vec<OadrEvent> {
        let intervals: Vec<_> = (0..4)
            .map(|i| {
                json!({
                    "id": i,
                    "intervalPeriod": {
                        "start": format!("2026-01-01T00:{:02}:00Z", i),
                        "duration": "PT60S"
                    },
                    "payloads": [{"type": "DEMAND", "values": [1.0]}]
                })
            })
            .collect();
        crate::controller::vtn_port::events_from_json(json!([{
            "id": "evt-1",
            "programID": "prog-1",
            "reportDescriptors": [descriptor],
            "intervals": intervals
        }]))
    }

    /// Four 15-minute intervals; the VTN asks for a report every 2 of them.
    ///
    /// This previously read `frequency` as a *second* count, so "every 2
    /// intervals" became "every 2 seconds" -- a VEN hammering the VTN roughly
    /// 900x more often than asked. 3.1 defines the field as a number of
    /// intervals ("-1 indicates same as numIntervals").
    #[test]
    fn report_cadence_counts_intervals_not_seconds() {
        let events = quarter_hourly_event(json!({
            "payloadType": "USAGE", "readingType": "DIRECT_READ", "frequency": 2
        }));
        let now = Utc::now();
        let obligations = extract_report_obligations(&events, now, &[]);
        assert_eq!(obligations.len(), 1);
        assert_eq!(
            obligations[0].submit_every_s, 1800,
            "2 intervals x PT15M = 1800s, not 2s"
        );
        assert_eq!(obligations[0].due_at, now + Duration::seconds(1800));
    }

    /// Absent `frequency` defers to `numIntervals`.
    #[test]
    fn report_cadence_without_frequency_uses_num_intervals() {
        let events = quarter_hourly_event(json!({
            "payloadType": "USAGE", "numIntervals": 3
        }));
        let obligations = extract_report_obligations(&events, Utc::now(), &[]);
        assert_eq!(obligations[0].submit_every_s, 2700);
    }

    /// Neither declared: one report covering every interval the event lists.
    #[test]
    fn report_cadence_defaults_to_the_whole_declared_sequence() {
        let events = quarter_hourly_event(json!({"payloadType": "USAGE"}));
        let obligations = extract_report_obligations(&events, Utc::now(), &[]);
        assert_eq!(obligations[0].submit_every_s, 3600, "4 x PT15M");
    }

    /// An event with no bounded interval gives nothing to count, so the
    /// documented profile default applies -- and is logged as applied.
    #[test]
    fn report_cadence_falls_back_when_the_event_declares_no_timing() {
        let events = json!([{
            "id": "evt-1",
            "programID": "prog-1",
            "reportDescriptors": [{"payloadType": "USAGE", "frequency": 2}],
            "intervals": []
        }]);
        let events = crate::controller::vtn_port::events_from_json(events);
        let obligations = extract_report_obligations(&events, Utc::now(), &[]);
        assert_eq!(obligations[0].submit_every_s, 3600);
    }

    /// Four contiguous PT15M intervals starting at a fixed instant.
    fn quarter_hourly_event(descriptor: serde_json::Value) -> Vec<OadrEvent> {
        let intervals: Vec<_> = (0..4)
            .map(|i| {
                json!({
                    "id": i,
                    "intervalPeriod": {
                        "start": format!("2026-01-01T{:02}:{:02}:00Z", i / 4, (i % 4) * 15),
                        "duration": "PT15M"
                    },
                    "payloads": [{"type": "USAGE", "values": [1.0]}]
                })
            })
            .collect();
        crate::controller::vtn_port::events_from_json(json!([{
            "id": "evt-1",
            "programID": "prog-1",
            "reportDescriptors": [descriptor],
            "intervals": intervals
        }]))
    }
}
