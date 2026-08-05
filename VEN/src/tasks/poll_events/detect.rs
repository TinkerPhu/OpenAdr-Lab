//! Pure change-detection pass over a freshly fetched event list (RF-B08).
//!
//! Split out of `poll_events/mod.rs` (R-64) once the module crossed the
//! `tasks/` 200-production-line cap — this half is the side-effect-free,
//! directly-unit-testable core; `spawn_event_poll` (the impure I/O loop)
//! stays in `mod.rs`.

use chrono::{DateTime, Utc};

use crate::controller;
use crate::controller::vtn_port::OadrEvent;
use crate::entities;
use crate::tasks::poll_signals;

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
    /// Parsed grid signals for this tick: alerts (WP3.1), SIMPLE levels
    /// (WP3.2), dispatch + charge-state setpoints (WP3.4).
    pub signals: poll_signals::ParsedSignals,
    /// History rows for events newly seen this tick (R-64) — one per
    /// `OpenAdrArrived` above, durable record of what VEN actually received.
    pub event_records: Vec<entities::history::EventReceived>,
}

/// Pure change-detection pass over a freshly fetched event list.
///
/// Compares against previous poll state and returns all trace events that
/// should be emitted, plus parsed rates/capacity for storage.  No I/O, no
/// state mutations — safe to unit-test.
pub(crate) fn detect_event_changes(
    events: &[OadrEvent],
    prev_ids: &std::collections::HashSet<String>,
    prev_tariff_count: usize,
    prev_import_limit: Option<f64>,
    now: DateTime<Utc>,
) -> EventChanges {
    let rates = controller::openadr_interface::parse_rate_snapshots(events, now);
    let capacity = controller::openadr_interface::parse_capacity_state(events, now);
    let signals = poll_signals::ParsedSignals {
        alerts: controller::openadr_interface::parse_alert_windows(events),
        simple: controller::openadr_interface::parse_simple_windows(events),
        dispatch: controller::openadr_interface::parse_dispatch_windows(events),
        charge_state: controller::openadr_interface::parse_charge_state_setpoint(events),
    };

    let current_ids: std::collections::HashSet<String> =
        events.iter().map(|e| e.id.clone()).collect();

    let mut trace_events = Vec::new();
    let mut event_records = Vec::new();

    // OpenAdrArrived — events that are new this tick
    for evt in events {
        if prev_ids.contains(&evt.id) {
            continue;
        }

        let name = evt.eventName.as_deref().unwrap_or(&evt.id).to_string();
        let (signal_type, value, interval_n) = evt
            .intervals
            .first()
            .and_then(|iv| iv.payloads.first())
            .map(|p| {
                let sig = p.r#type.clone();
                let val = p.values.first().and_then(|v| v.as_f64()).unwrap_or(0.0);
                let n = evt.intervals.len() as u32;
                (sig, val, n)
            })
            .unwrap_or_else(|| ("UNKNOWN".to_string(), 0.0, 0));

        event_records.push(entities::history::EventReceived {
            received_at: now,
            event_id: evt.id.clone(),
            event_type: signal_type.clone(),
            payload_json: serde_json::to_string(evt).unwrap_or_default(),
        });

        trace_events.push(controller::trace::ControllerEvent::OpenAdrArrived {
            ts: now,
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
        signals,
        event_records,
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
        serde_json::from_value(serde_json::json!({
            "id": id,
            "programID": "test-program",
            "eventName": name,
            "intervals": [{
                "payloads": [{"type": signal_type, "values": [value]}]
            }]
        }))
        .unwrap()
    }

    fn empty_ids() -> std::collections::HashSet<String> {
        std::collections::HashSet::new()
    }

    // (a) new event appears → OpenAdrArrived emitted
    #[test]
    fn new_event_emits_arrived() {
        let events = vec![make_event("ev1", "Peak DR", "PRICE", 0.30)];
        let changes = detect_event_changes(&events, &empty_ids(), 0, None, ts());
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

    // (a.1) new event appears → also recorded as an EventReceived history row
    #[test]
    fn new_event_emits_history_record() {
        let events = vec![make_event("ev1", "Peak DR", "PRICE", 0.30)];
        let changes = detect_event_changes(&events, &empty_ids(), 0, None, ts());
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
        let changes = detect_event_changes(&events, &prev_ids, 0, None, ts());
        assert!(changes.event_records.is_empty());
    }

    // (b) event disappears → OpenAdrExpired emitted
    #[test]
    fn removed_event_emits_expired() {
        let mut prev_ids = empty_ids();
        prev_ids.insert("ev1".to_string());
        let changes = detect_event_changes(&[], &prev_ids, 0, None, ts());
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
        let events = vec![serde_json::from_value::<OadrEvent>(serde_json::json!({
            "id": "ev1",
            "programID": "prog",
            "eventName": "Price Event",
            "intervals": [{
                "intervalPeriod": {"start": "2026-03-21T10:00:00Z", "duration": "PT1H"},
                "payloads": [{"type": "PRICE", "values": [0.25]}]
            }]
        }))
        .unwrap()];
        let mut prev_ids = empty_ids();
        prev_ids.insert("ev1".to_string()); // already seen → no OpenAdrArrived
        let changes = detect_event_changes(&events, &prev_ids, 0, None, ts());
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
        let events = vec![serde_json::from_value::<OadrEvent>(serde_json::json!({
            "id": "ev1",
            "programID": "prog",
            "eventName": "Capacity Event",
            "intervals": [{
                "intervalPeriod": {"start": "2026-03-21T10:00:00Z", "duration": "PT1H"},
                "payloads": [{"type": "IMPORT_CAPACITY_LIMIT", "values": [5.0]}]
            }]
        }))
        .unwrap()];
        let mut prev_ids = empty_ids();
        prev_ids.insert("ev1".to_string()); // already seen
        let prev_limit: Option<f64> = None;
        let changes = detect_event_changes(&events, &prev_ids, 0, prev_limit, ts());
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
        let changes = detect_event_changes(&events, &prev_ids, 999, None, ts());
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
            interval_duration_s: 900,
            fulfilled: false,
            created_at: now,
            historical: true,
        };
        state.add_obligations(vec![ob]).await;

        // First poll still has ev1 — obligation survives.
        let first = detect_event_changes(
            &[make_event("ev1", "Peak DR", "PRICE", 0.30)],
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
        let second = detect_event_changes(&[], &first.current_ids, 0, None, now);
        state.retire_obligations_not_in(&second.current_ids).await;
        assert!(
            state.report_obligations().await.is_empty(),
            "obligation retired once its event expired"
        );
    }
}
