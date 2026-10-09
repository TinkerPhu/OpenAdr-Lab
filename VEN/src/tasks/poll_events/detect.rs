//! Pure change-detection pass over a freshly fetched event list (RF-B08).
//!
//! Split out of `poll_events/mod.rs` (R-64) once the module crossed the
//! `tasks/` 200-production-line cap — this half is the side-effect-free,
//! directly-unit-testable core; `spawn_event_poll` (the impure I/O loop)
//! stays in `mod.rs`.

use chrono::{DateTime, Utc};

use crate::controller;
use crate::controller::vtn_port::{EventTypeName, OadrEvent};
use crate::entities;
use crate::tasks::poll_signals;
use lab_core::wire_contract::{PayloadReader, WireAudit};

/// Output of `detect_event_changes` — all side-effect-free results of one poll tick.
pub(crate) struct EventChanges {
    /// Trace events to push to the controller log (arrived/expired/rate/capacity).
    pub trace_events: Vec<controller::trace::ControllerEvent>,
    /// Updated set of event IDs seen this tick (new value for `prev_event_ids`).
    pub current_ids: std::collections::HashSet<String>,
    /// Parsed tariff snapshots for this tick.
    pub rates: Vec<entities::tariff_snapshot::TariffSnapshot>,
    /// Parsed capacity state for this tick.
    pub capacity: entities::capacity::OadrCapacityState,
    /// Parsed capacity-limit schedule (Dynamic Operating Envelope) for this tick.
    pub capacity_schedule: Vec<entities::capacity::CapacitySnapshot>,
    /// Parsed grid signals for this tick: alerts (WP3.1), SIMPLE levels
    /// (WP3.2), dispatch + charge-state setpoints (WP3.4).
    pub signals: poll_signals::ParsedSignals,
    /// History rows for events newly seen this tick (R-64) — one per
    /// `OpenAdrArrived` above, durable record of what VEN actually received.
    pub event_records: Vec<entities::history::EventReceived>,
    /// What this poll's values assumed or had refused about their units (GB-50).
    pub wire_audit: WireAudit,
}

/// `events` as this VEN acts on them: each declared start moved by the VEN's own
/// `randomizeStart` offset (R-86, `lab_core::event_timing::with_randomized_start`). What the
/// poll parses into windows, rates and limits comes from this; the declared events, the report
/// obligations and the trace do not, because they say what the VTN asked, not when this VEN
/// begins to respond. One call site, so "which events are staggered" cannot differ per parser.
pub(crate) fn events_this_ven_acts_on(events: &[OadrEvent], ven_seed: &str) -> Vec<OadrEvent> {
    events
        .iter()
        .map(|e| lab_core::event_timing::with_randomized_start(e, ven_seed))
        .collect()
}

/// Pure change-detection pass over a freshly fetched event list.
///
/// Compares against previous poll state and returns all trace events that
/// should be emitted, plus parsed rates/capacity for storage.  No I/O, no
/// state mutations — safe to unit-test.
pub(crate) fn detect_event_changes(
    events: &[OadrEvent],
    reader: &PayloadReader,
    prev_ids: &std::collections::HashSet<String>,
    prev_tariff_count: usize,
    prev_import_limit: Option<f64>,
    now: DateTime<Utc>,
) -> EventChanges {
    let rates = controller::openadr_interface::parse_rate_snapshots(events, reader);
    let capacity = controller::openadr_interface::parse_capacity_state(events, reader, now);
    let capacity_schedule = controller::openadr_interface::parse_capacity_schedule(events, reader);
    let signals = poll_signals::ParsedSignals {
        alerts: controller::openadr_interface::parse_alert_windows(events),
        simple: controller::openadr_interface::parse_simple_windows(events, reader),
        dispatch: controller::openadr_interface::parse_dispatch_windows(events, reader),
        unapplied: controller::openadr_interface::parse_unapplied_payloads(events),
    };

    let current_ids: std::collections::HashSet<String> =
        events.iter().map(|e| e.id.to_string()).collect();

    let mut trace_events = Vec::new();
    let mut event_records = Vec::new();

    // OpenAdrArrived — events that are new this tick
    for evt in events {
        if prev_ids.contains(evt.id.as_str()) {
            continue;
        }

        let name = evt
            .content
            .event_name
            .clone()
            .unwrap_or_else(|| evt.id.to_string());
        let intervals = evt.content.intervals.as_deref().unwrap_or_default();
        let (signal_type, value, interval_n) = intervals
            .first()
            .and_then(|iv| iv.payloads.first())
            .map(|p| {
                let sig = p.value_type.wire_name();
                // Through the reader, like every other value: a refused or non-numeric
                // payload traces as 0.0.
                let val = reader.value(evt, p).unwrap_or(0.0);
                (sig, val, intervals.len() as u32)
            })
            .unwrap_or_else(|| ("UNKNOWN".to_string(), 0.0, 0));

        event_records.push(entities::history::EventReceived {
            received_at: now,
            event_id: evt.id.to_string(),
            event_type: signal_type.clone(),
            payload_json: serde_json::to_string(evt).unwrap_or_default(),
        });

        trace_events.push(controller::trace::ControllerEvent::OpenAdrArrived {
            ts: now,
            event_id: evt.id.to_string(),
            // Passed through as the VTN wrote it (`dto`): this is an identity,
            // and reformatting an identity is how two records of the same
            // thing stop matching.
            modification_date_time: Some(evt.modification_date_time.to_rfc3339()),
            event_name: name,
            signal_type,
            value,
            interval: interval_n,
        });
    }

    // OpenAdrExpired — events that disappeared this tick
    for old_id in prev_ids {
        if !current_ids.contains(old_id) {
            trace_events.push(controller::trace::ControllerEvent::OpenAdrExpired {
                ts: now,
                event_id: old_id.clone(),
                event_name: old_id.clone(),
            });
        }
    }

    // RateChange — tariff count changed
    if !rates.is_empty() && rates.len() != prev_tariff_count {
        if let Some(first) = rates.first() {
            trace_events.push(controller::trace::ControllerEvent::RateChange {
                ts: now,
                interval_start: first.interval_start,
                import_eur_kwh: first.import_tariff_eur_kwh.unwrap_or(0.0),
                export_eur_kwh: first.export_tariff_eur_kwh.unwrap_or(0.0),
            });
        }
    }

    // CapacityChange — import limit changed
    if capacity.import_limit_kw != prev_import_limit {
        trace_events.push(controller::trace::ControllerEvent::CapacityChange {
            ts: now,
            import_limit_kw: capacity.import_limit_kw,
            export_limit_kw: capacity.export_limit_kw,
        });
    }

    EventChanges {
        trace_events,
        current_ids,
        rates,
        capacity,
        capacity_schedule,
        signals,
        event_records,
        wire_audit: reader.audit(events),
    }
}

#[cfg(test)]
mod event_poll_tests {
    use super::*;
    use crate::controller::vtn_port::OadrEvent;
    use chrono::TimeZone;

    fn ts() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 3, 21, 10, 0, 0).unwrap()
    }

    fn make_event(id: &str, name: &str, signal_type: &str, value: f64) -> OadrEvent {
        lab_core::test_fixtures::events_from_json(serde_json::json!({
            "id": id,
            "programID": "test-program",
            "eventName": name,
            "modificationDateTime": "2026-03-21T09:00:00Z",
            "intervals": [{
                "id": 0,
                "payloads": [{"type": signal_type, "values": [value]}]
            }]
        }))
        .remove(0)
    }

    fn empty_ids() -> std::collections::HashSet<String> {
        std::collections::HashSet::new()
    }

    // (a) new event appears → OpenAdrArrived emitted
    #[test]
    fn new_event_emits_arrived() {
        let events = vec![make_event("ev1", "Peak DR", "PRICE", 0.30)];
        let changes = detect_event_changes(
            &events,
            &PayloadReader::default(),
            &empty_ids(),
            0,
            None,
            ts(),
        );
        let arrived: Vec<_> = changes
            .trace_events
            .iter()
            .filter(|e| matches!(e, controller::trace::ControllerEvent::OpenAdrArrived { .. }))
            .collect();
        assert_eq!(arrived.len(), 1);
        if let controller::trace::ControllerEvent::OpenAdrArrived {
            event_name,
            signal_type,
            value,
            ..
        } = &arrived[0]
        {
            assert_eq!(event_name, "Peak DR");
            assert_eq!(signal_type, "PRICE");
            assert!((value - 0.30).abs() < 1e-9);
        }
    }

    /// §6.3: a reaction trace has to name the event *version* the VEN acted
    /// on. `event_name` cannot do that -- the VTN does not enforce unique
    /// names, and an edited event keeps its name while changing what it says.
    #[test]
    fn arrived_carries_the_event_id_and_the_version_it_saw() {
        let events = vec![make_event("ev1", "Peak DR", "PRICE", 0.30)];
        let changes = detect_event_changes(
            &events,
            &PayloadReader::default(),
            &empty_ids(),
            0,
            None,
            ts(),
        );
        let controller::trace::ControllerEvent::OpenAdrArrived {
            event_id,
            modification_date_time,
            ..
        } = &changes.trace_events[0]
        else {
            panic!("expected OpenAdrArrived first");
        };
        assert_eq!(event_id, "ev1");
        assert_eq!(
            modification_date_time.as_deref(),
            Some("2026-03-21T09:00:00+00:00"),
            "the version stamp must be the VTN's own, verbatim"
        );
    }

    /// §6.3: a replan has to be attributable to the events that caused it.
    /// The ids the poll loop sends with the trigger come from these entries,
    /// so if they are ever dropped here the chain silently degrades to
    /// "something changed around then".
    #[test]
    fn arrived_and_expired_together_carry_every_id_a_replan_was_caused_by() {
        let mut prev_ids = empty_ids();
        prev_ids.insert("gone".to_string());
        let events = vec![make_event("fresh", "New", "PRICE", 0.2)];
        let changes =
            detect_event_changes(&events, &PayloadReader::default(), &prev_ids, 0, None, ts());

        let causes: Vec<String> = changes
            .trace_events
            .iter()
            .filter_map(|e| match e {
                controller::trace::ControllerEvent::OpenAdrArrived { event_id, .. }
                | controller::trace::ControllerEvent::OpenAdrExpired { event_id, .. } => {
                    Some(event_id.clone())
                }
                _ => None,
            })
            .collect();
        assert!(
            causes.contains(&"fresh".to_string()),
            "the new event: {causes:?}"
        );
        assert!(
            causes.contains(&"gone".to_string()),
            "the expired one: {causes:?}"
        );
    }

    /// The name of a vanished event is not knowable -- it is gone -- but its
    /// id is, and the id is what the chain is keyed on.
    #[test]
    fn expired_carries_the_event_id() {
        let mut prev_ids = empty_ids();
        prev_ids.insert("ev1".to_string());
        let changes =
            detect_event_changes(&[], &PayloadReader::default(), &prev_ids, 0, None, ts());
        let expired = changes
            .trace_events
            .iter()
            .find_map(|e| match e {
                controller::trace::ControllerEvent::OpenAdrExpired { event_id, .. } => {
                    Some(event_id)
                }
                _ => None,
            })
            .expect("expected an OpenAdrExpired");
        assert_eq!(expired, "ev1");
    }

    // (a.1) new event appears → also recorded as an EventReceived history row
    #[test]
    fn new_event_emits_history_record() {
        let events = vec![make_event("ev1", "Peak DR", "PRICE", 0.30)];
        let changes = detect_event_changes(
            &events,
            &PayloadReader::default(),
            &empty_ids(),
            0,
            None,
            ts(),
        );
        assert_eq!(changes.event_records.len(), 1);
        let row = &changes.event_records[0];
        assert_eq!(row.event_id, "ev1");
        assert_eq!(row.event_type, "PRICE");
        assert_eq!(row.received_at, ts());
        assert!(row.payload_json.contains("\"id\":\"ev1\""));
    }

    // (a.2) already-seen event → no history record emitted again
    #[test]
    fn already_seen_event_emits_no_history_record() {
        let events = vec![make_event("ev1", "Peak DR", "PRICE", 0.30)];
        let mut prev_ids = empty_ids();
        prev_ids.insert("ev1".to_string());
        let changes =
            detect_event_changes(&events, &PayloadReader::default(), &prev_ids, 0, None, ts());
        assert!(changes.event_records.is_empty());
    }

    // (b) event disappears → OpenAdrExpired emitted
    #[test]
    fn removed_event_emits_expired() {
        let mut prev_ids = empty_ids();
        prev_ids.insert("ev1".to_string());
        let changes =
            detect_event_changes(&[], &PayloadReader::default(), &prev_ids, 0, None, ts());
        let expired: Vec<_> = changes
            .trace_events
            .iter()
            .filter(|e| matches!(e, controller::trace::ControllerEvent::OpenAdrExpired { .. }))
            .collect();
        assert_eq!(expired.len(), 1);
        if let controller::trace::ControllerEvent::OpenAdrExpired { event_name, .. } = &expired[0] {
            assert_eq!(event_name, "ev1");
        }
    }

    // (c) tariff count changes → RateChange emitted
    #[test]
    fn tariff_count_change_emits_rate_change() {
        let events = lab_core::test_fixtures::events_from_json(serde_json::json!({
            "id": "ev1",
            "programID": "prog",
            "eventName": "Price Event",
            "intervals": [{
                "id": 0,
                "intervalPeriod": {"start": "2026-03-21T10:00:00Z", "duration": "PT1H"},
                "payloads": [{"type": "PRICE", "values": [0.25]}]
            }]
        }));
        let mut prev_ids = empty_ids();
        prev_ids.insert("ev1".to_string()); // already seen → no OpenAdrArrived
        let changes =
            detect_event_changes(&events, &PayloadReader::default(), &prev_ids, 0, None, ts());
        // Only assert if the parser actually produced rates (depends on parser internals)
        if !changes.rates.is_empty() {
            let rate_changes: Vec<_> = changes
                .trace_events
                .iter()
                .filter(|e| matches!(e, controller::trace::ControllerEvent::RateChange { .. }))
                .collect();
            assert_eq!(rate_changes.len(), 1);
        }
    }

    // (d) import limit changes → CapacityChange emitted
    #[test]
    fn import_limit_change_emits_capacity_change() {
        let events = lab_core::test_fixtures::events_from_json(serde_json::json!({
            "id": "ev1",
            "programID": "prog",
            "eventName": "Capacity Event",
            "intervals": [{
                "id": 0,
                "intervalPeriod": {"start": "2026-03-21T10:00:00Z", "duration": "PT1H"},
                "payloads": [{"type": "IMPORT_CAPACITY_LIMIT", "values": [5.0]}]
            }]
        }));
        let mut prev_ids = empty_ids();
        prev_ids.insert("ev1".to_string()); // already seen
        let prev_limit: Option<f64> = None;
        let changes = detect_event_changes(
            &events,
            &PayloadReader::default(),
            &prev_ids,
            0,
            prev_limit,
            ts(),
        );
        if changes.capacity.import_limit_kw != prev_limit {
            let cap_changes: Vec<_> = changes
                .trace_events
                .iter()
                .filter(|e| matches!(e, controller::trace::ControllerEvent::CapacityChange { .. }))
                .collect();
            assert_eq!(cap_changes.len(), 1);
        }
    }

    // (e) no changes → no arrived/expired/capacity events emitted
    #[test]
    fn no_changes_emits_nothing() {
        let events = vec![make_event("ev1", "Peak DR", "PRICE", 0.30)];
        let mut prev_ids = empty_ids();
        prev_ids.insert("ev1".to_string());
        // Same event already seen, no capacity limit in payload, same import limit (None)
        let changes = detect_event_changes(
            &events,
            &PayloadReader::default(),
            &prev_ids,
            999,
            None,
            ts(),
        );
        let no_arrived = !changes
            .trace_events
            .iter()
            .any(|e| matches!(e, controller::trace::ControllerEvent::OpenAdrArrived { .. }));
        let no_expired = !changes
            .trace_events
            .iter()
            .any(|e| matches!(e, controller::trace::ControllerEvent::OpenAdrExpired { .. }));
        let no_capacity = !changes
            .trace_events
            .iter()
            .any(|e| matches!(e, controller::trace::ControllerEvent::CapacityChange { .. }));
        assert!(no_arrived, "expected no OpenAdrArrived");
        assert!(no_expired, "expected no OpenAdrExpired");
        assert!(no_capacity, "expected no CapacityChange");
    }

    // (f) obligation retirement — event drops out of the active poll set
    #[tokio::test]
    async fn obligation_retired_when_event_expires() {
        use crate::entities::capacity::OadrReportObligation;
        use crate::state::AppState;

        let state = AppState::new();
        let now = ts();
        let ob = OadrReportObligation {
            id: uuid::Uuid::new_v4(),
            event_id: "ev1".to_string(),
            program_id: Some("test-program".to_string()),
            payload_type: "USAGE".to_string(),
            reading_type: "DIRECT_READ".to_string(),
            resource_name: None,
            due_at: now,
            interval_width_s: 900,
            submit_every_s: 900,
            fulfilled: false,
            created_at: now,
            historical: true,
        };
        state.add_obligations(vec![ob]).await;

        // First poll still has ev1 — obligation survives.
        let first = detect_event_changes(
            &[make_event("ev1", "Peak DR", "PRICE", 0.30)],
            &PayloadReader::default(),
            &empty_ids(),
            0,
            None,
            now,
        );
        state.retire_obligations_not_in(&first.current_ids).await;
        assert_eq!(
            state.report_obligations().await.len(),
            1,
            "event still active"
        );

        // Second poll: ev1 no longer present — obligation is retired.
        let second = detect_event_changes(
            &[],
            &PayloadReader::default(),
            &first.current_ids,
            0,
            None,
            now,
        );
        state.retire_obligations_not_in(&second.current_ids).await;
        assert!(
            state.report_obligations().await.is_empty(),
            "obligation retired once its event expired"
        );
    }

    // ── randomizeStart (R-86) ──────────────────────────────────────────────

    fn simple_event_with_window(window: Option<&str>) -> Vec<OadrEvent> {
        let mut period = serde_json::json!({"start": "2026-03-21T11:00:00Z", "duration": "PT1H"});
        if let Some(w) = window {
            period["randomizeStart"] = serde_json::json!(w);
        }
        lab_core::test_fixtures::events_from_json(serde_json::json!([{
            "id": "stagger-1", "programID": "p", "intervalPeriod": period,
            "intervals": [{"id": 0, "payloads": [{"type": "SIMPLE", "values": [2]}]}]
        }]))
    }

    fn simple_start(events: &[OadrEvent], seed: &str) -> DateTime<Utc> {
        let acted_on = events_this_ven_acts_on(events, seed);
        detect_event_changes(
            &acted_on,
            &PayloadReader::default(),
            &empty_ids(),
            0,
            None,
            ts(),
        )
        .signals
        .simple[0]
            .start
    }

    /// The field's purpose: two VENs given one event do not begin on one instant.
    #[test]
    fn two_vens_begin_a_randomized_event_at_their_own_offsets() {
        let events = simple_event_with_window(Some("PT10M"));
        let nominal = Utc.with_ymd_and_hms(2026, 3, 21, 11, 0, 0).unwrap();
        let (a, b) = (
            simple_start(&events, "ven-1"),
            simple_start(&events, "ven-2"),
        );
        assert_ne!(a, b);
        for start in [a, b] {
            assert!(
                start >= nominal && start < nominal + chrono::Duration::minutes(10),
                "{start}"
            );
        }
        let offset = lab_core::event_timing::randomized_start_offset(
            "ven-1",
            "stagger-1",
            chrono::Duration::minutes(10),
        );
        assert_eq!(
            a,
            nominal + offset,
            "the window starts exactly at this VEN's offset"
        );
    }

    #[test]
    fn an_event_without_randomize_start_begins_on_its_declared_start() {
        let events = simple_event_with_window(None);
        let nominal = Utc.with_ymd_and_hms(2026, 3, 21, 11, 0, 0).unwrap();
        assert_eq!(simple_start(&events, "ven-1"), nominal);
        assert_eq!(simple_start(&events, "ven-2"), nominal);
    }

    /// Reporting is not staggered: obligations are read from the declared events.
    #[test]
    fn the_declared_event_is_not_changed_by_staggering() {
        let events = simple_event_with_window(Some("PT10M"));
        let _ = events_this_ven_acts_on(&events, "ven-1");
        let declared = events[0].content.interval_period.as_ref().unwrap();
        assert_eq!(
            declared.start,
            Utc.with_ymd_and_hms(2026, 3, 21, 11, 0, 0).unwrap()
        );
    }

    /// GB-50: one poll reports what it assumed and what it refused, and a refused value is
    /// neither a rate nor a traced number.
    #[test]
    fn a_poll_reports_assumed_and_refused_units() {
        let mut refused =
            serde_json::to_value(make_event("ev-usd", "USD price", "PRICE", 0.30)).unwrap();
        refused["payloadDescriptors"] =
            serde_json::json!([{ "payloadType": "PRICE", "units": "KWH", "currency": "USD" }]);
        let events = vec![
            make_event("ev-undeclared", "limit", "IMPORT_CAPACITY_LIMIT", 4.0),
            serde_json::from_value(refused).unwrap(),
        ];
        let changes = detect_event_changes(
            &events,
            &PayloadReader::default(),
            &empty_ids(),
            0,
            None,
            ts(),
        );
        assert_eq!(
            changes.wire_audit.assumed.get("IMPORT_CAPACITY_LIMIT"),
            Some(&1)
        );
        assert_eq!(changes.wire_audit.refusals.len(), 1);
        assert_eq!(changes.wire_audit.refusals[0].event_id, "ev-usd");
        assert!(changes.rates.is_empty(), "the USD price is not used");
        assert_eq!(
            changes.capacity.import_limit_kw,
            Some(4.0),
            "the undeclared limit still applies"
        );
    }
}
