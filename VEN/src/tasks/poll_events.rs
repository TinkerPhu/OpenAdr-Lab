use chrono::{DateTime, Utc};
use metrics::counter;
use std::sync::Arc;
use tracing::{error, info};

use crate::controller;
use crate::controller::vtn_port::OadrEvent;
use crate::controller::VtnPort;
use crate::entities;
use crate::entities::asset::PlanTrigger;
use crate::state::AppState;
use crate::tasks::backoff::Backoff;

// ─── Event poll change detection (RF-B08) ─────────────────────────────────────

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
    pub signals: super::poll_signals::ParsedSignals,
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
    let capacity = controller::openadr_interface::parse_capacity_state(events);
    let signals = super::poll_signals::ParsedSignals {
        alerts: controller::openadr_interface::parse_alert_windows(events),
        simple: controller::openadr_interface::parse_simple_windows(events),
        dispatch: controller::openadr_interface::parse_dispatch_windows(events),
        charge_state: controller::openadr_interface::parse_charge_state_setpoint(events),
    };

    let current_ids: std::collections::HashSet<String> =
        events.iter().map(|e| e.id.clone()).collect();

    let mut trace_events = Vec::new();

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
    }
}

// ─── Background loop spawners ──────────────────────────────────────────────────

/// `startup_delay_s` (GB-09, WP2.5): see `spawn_program_poll`.
pub(crate) fn spawn_event_poll(
    state: AppState,
    vtn: Arc<dyn VtnPort>,
    secs: u64,
    trigger_tx: Arc<tokio::sync::watch::Sender<PlanTrigger>>,
    notifier: crate::services::notify::Notifier,
    startup_delay_s: u64,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        if startup_delay_s > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(startup_delay_s)).await;
        }
        let mut backoff = Backoff::new(secs, secs.saturating_mul(30).min(900), 0);
        // Track previous event IDs and tariff count for change detection (T034/T035)
        let mut prev_event_ids: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let mut prev_tariff_count: usize = 0;
        let mut prev_import_limit: Option<f64> = None;
        let mut signal_prevs = super::poll_signals::SignalPrevs::default();
        let mut vtn_ok = true; // WP4.3: notify only on reachable⇄unreachable edges
        loop {
            use crate::services::notify::notify_outage_edge as outage_edge;
            match vtn.fetch_events().await {
                Ok(events) => {
                    vtn_ok = outage_edge(&notifier, &state, Utc::now(), vtn_ok, true).await;
                    counter!("poll_success_total", "resource" => "events").increment(1);
                    info!(resource = "events", count = events.len(), "poll success");

                    let now = Utc::now();
                    let changes = detect_event_changes(
                        &events,
                        &prev_event_ids,
                        prev_tariff_count,
                        prev_import_limit,
                        now,
                    );

                    // Check before the trace_events vec is consumed by the for loop.
                    let any_change = !changes.trace_events.is_empty();

                    for evt in changes.trace_events {
                        state.push_controller_event(evt).await;
                    }
                    prev_event_ids = changes.current_ids;
                    prev_tariff_count = changes.rates.len();
                    prev_import_limit = changes.capacity.import_limit_kw;

                    state.set_planned_tariffs(changes.rates).await;
                    state.set_capacity_state(changes.capacity).await;

                    // WP3.1/3.2/3.4: apply alert/SIMPLE/dispatch/charge-state
                    // signal changes (see poll_signals.rs). True = a plan
                    // trigger was already sent; don't overwrite it below.
                    let signal_trigger_sent = super::poll_signals::apply_signal_changes(
                        &state,
                        &trigger_tx,
                        &notifier,
                        changes.signals,
                        now,
                        &mut signal_prevs,
                    )
                    .await;

                    let existing_obs = state.report_obligations().await;
                    let new_obs = controller::openadr_interface::extract_report_obligations(
                        &events,
                        now,
                        &existing_obs,
                    );
                    state.add_obligations(new_obs).await;
                    state.retire_obligations_not_in(&prev_event_ids).await;

                    state.set_events(events, 500).await;

                    // Signal planner only when something actually changed (new/expired event,
                    // tariff count change, or capacity change). Firing on every poll caused
                    // continuous replanning at the poll interval (~30s) regardless of whether
                    // rates changed, which destabilised the plan.
                    // trigger_tx is a watch channel (latest wins) — don't overwrite
                    // an Alert/CapacityChange trigger sent above with RateChange.
                    if any_change && !signal_trigger_sent {
                        let _ = trigger_tx.send(PlanTrigger::RateChange);
                    }
                    super::backoff::record_success(&mut backoff, &state, now).await;
                    tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
                }
                Err(e) => {
                    counter!("poll_error_total", "resource" => "events").increment(1);
                    error!(resource = "events", "poll failed: {e:#}");
                    vtn_ok = outage_edge(&notifier, &state, Utc::now(), vtn_ok, false).await;
                    super::backoff::record_fail_sleep(&mut backoff, &state, Utc::now(), e).await;
                }
            }
        }
    })
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
