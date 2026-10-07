//! Application of parsed grid-signal changes from one event poll (split out
//! of `poll_events.rs` for the tasks/ file-size cap): alert windows (WP3.1),
//! SIMPLE levels (WP3.2), direct setpoints (WP3.4), and the announcement of payloads this
//! profile does not apply (R-100). Pure change-vs-prev
//! bookkeeping plus state writes and plan triggers — no parsing here.

use chrono::{DateTime, Utc};

use crate::controller;
use crate::entities::asset::{PlanTrigger, PlanTriggerSignal};
use crate::entities::capacity::{AlertWindow, DispatchWindow, SimpleWindow};
use crate::state::AppState;

/// The signal payloads parsed from one poll's event list.
#[derive(Default)]
pub(crate) struct ParsedSignals {
    pub alerts: Vec<AlertWindow>,
    pub simple: Vec<SimpleWindow>,
    pub dispatch: Vec<DispatchWindow>,
    /// Events carrying a payload this profile does not apply (R-100).
    pub unapplied: Vec<controller::openadr_interface::UnappliedPayload>,
}

/// Previous-poll signal state, owned by the poll loop across iterations.
#[derive(Default)]
pub(crate) struct SignalPrevs {
    pub alerts: Vec<AlertWindow>,
    pub simple: Vec<SimpleWindow>,
    pub dispatch: Vec<DispatchWindow>,
    /// What was already announced as not applied, so a standing event is said once.
    pub unapplied: Vec<controller::openadr_interface::UnappliedPayload>,
}

/// The events this VEN starts later than declared (R-86), with the delay. `acted_on` is
/// `events_this_ven_acts_on(declared, ..)`, so the two lists line up one to one.
pub(crate) fn staggered_starts(
    declared: &[crate::controller::vtn_port::OadrEvent],
    acted_on: &[crate::controller::vtn_port::OadrEvent],
) -> Vec<(String, chrono::Duration)> {
    declared
        .iter()
        .zip(acted_on)
        .filter_map(|(d, a)| {
            let shift = a.content.interval_period.as_ref()?.start
                - d.content.interval_period.as_ref()?.start;
            (shift > chrono::Duration::zero()).then(|| (d.id.to_string(), shift))
        })
        .collect()
}

/// Say, once per event, that this VEN starts it later than declared: a window that opens
/// minutes after the VTN's start would otherwise look like a bug (`ui-transparency`).
pub(crate) async fn announce_staggered_starts(
    state: &AppState,
    notifier: &crate::services::notify::Notifier,
    declared: &[crate::controller::vtn_port::OadrEvent],
    acted_on: &[crate::controller::vtn_port::OadrEvent],
    now: DateTime<Utc>,
    announced: &mut std::collections::HashSet<String>,
) {
    for (event_id, shift) in staggered_starts(declared, acted_on) {
        if !announced.insert(event_id.clone()) {
            continue;
        }
        notifier
            .notify(
                state,
                now,
                crate::entities::design_vocabulary::UserNotificationSeverity::Info,
                format!(
                    "randomizeStart: this VEN begins the event {} s after its declared start",
                    shift.num_seconds()
                ),
                None,
                Some(event_id.clone()),
                Some(format!("randomize-start-{event_id}")),
            )
            .await;
    }
}

/// Apply this poll's parsed signals. Returns `true` when a plan trigger was
/// already sent (Alert / CapacityChange / UserRequest) — the caller must then
/// not overwrite it with RateChange, since `trigger_tx` is a watch channel
/// where only the latest value survives.
pub(crate) async fn apply_signal_changes(
    state: &AppState,
    trigger_tx: &tokio::sync::watch::Sender<PlanTriggerSignal>,
    notifier: &crate::services::notify::Notifier,
    signals: ParsedSignals,
    now: DateTime<Utc>,
    prevs: &mut SignalPrevs,
) -> bool {
    let ParsedSignals {
        alerts,
        simple,
        dispatch,
        unapplied,
    } = signals;
    // WP3.1 (BL-04): alert changes replan with the Alert trigger.
    let alerts_changed = alerts != prevs.alerts;
    if alerts_changed {
        state.set_alert_windows(alerts.clone()).await;
        // WP4.3 (BL-20): each newly-appearing alert window is a grid
        // emergency the resident should see — notify once per window.
        for w in alerts.iter().filter(|w| !prevs.alerts.contains(w)) {
            notifier
                .notify(
                    state,
                    now,
                    crate::entities::design_vocabulary::UserNotificationSeverity::Alert,
                    format!("Grid emergency ({}): {}", w.alert_type, w.message),
                    None,
                    Some(w.event_id.clone()),
                    None,
                )
                .await;
        }
        prevs.alerts = alerts;
        // The alert windows name their own events, so the replan they cause
        // can be attributed to them (§6.3).
        let _ = trigger_tx.send(PlanTriggerSignal::caused_by(
            PlanTrigger::Alert,
            prevs.alerts.iter().map(|w| w.event_id.clone()).collect(),
        ));
    }

    // WP3.2: SIMPLE changes replan as CapacityChange (they constrain the
    // same per-slot import cap); Alert wins the label if both changed.
    let simple_changed = simple != prevs.simple;
    if simple_changed {
        state.set_simple_windows(simple.clone()).await;
        prevs.simple = simple;
        if !alerts_changed {
            let _ = trigger_tx.send(PlanTriggerSignal::caused_by(
                PlanTrigger::CapacityChange,
                prevs.simple.iter().map(|w| w.event_id.clone()).collect(),
            ));
        }
    }

    // WP3.4: dispatch windows steer the tick dispatcher directly (no replan —
    // the plan keeps running underneath); trace active/cleared transitions.
    if dispatch != prevs.dispatch {
        let active = !dispatch.is_empty();
        let setpoint_kw = dispatch.first().map(|w| w.setpoint_kw);
        state.set_dispatch_windows(dispatch.clone()).await;
        prevs.dispatch = dispatch;
        state
            .push_controller_event(controller::trace::ControllerEvent::DispatchOverride {
                ts: now,
                setpoint_kw,
                active,
            })
            .await;
    }

    // R-100: a payload this profile does not apply is said so, once per event, rather than
    // dropped in silence. Info, not a wire rejection: the VTN did nothing wrong and `/health`
    // must not read as degraded because of a stated policy.
    for u in unapplied.iter().filter(|u| !prevs.unapplied.contains(u)) {
        notifier
            .notify(
                state,
                now,
                crate::entities::design_vocabulary::UserNotificationSeverity::Info,
                format!(
                    "{} in this event is not applied: {}",
                    u.payload_type, u.reason
                ),
                None,
                Some(u.event_id.clone()),
                Some(format!(
                    "payload-not-applied-{}-{}",
                    u.payload_type, u.event_id
                )),
            )
            .await;
    }
    prevs.unapplied = unapplied;

    alerts_changed || simple_changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn ts(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000 + secs, 0).unwrap()
    }

    fn unapplied(event_id: &str) -> controller::openadr_interface::UnappliedPayload {
        controller::openadr_interface::UnappliedPayload {
            payload_type: "CHARGE_STATE_SETPOINT".to_string(),
            event_id: event_id.to_string(),
            reason: "the driver decides how a household EV charges".to_string(),
        }
    }

    /// R-100: a payload this profile does not apply is announced - once per event - and never
    /// becomes an EV session. The announcement is what makes "not applied" a stated policy
    /// rather than a silent drop (`wire-contracts`).
    #[tokio::test]
    async fn an_unapplied_payload_is_announced_once_and_creates_no_ev_session() {
        use crate::entities::design_vocabulary::UserNotificationSeverity;
        let state = AppState::new();
        let (tx, _rx) = tokio::sync::watch::channel(PlanTriggerSignal::bare(PlanTrigger::Periodic));
        let mut prevs = SignalPrevs::default();
        let notifier = crate::services::notify::Notifier::new(None);

        for secs in [0, 30] {
            let signals = ParsedSignals {
                unapplied: vec![unapplied("evt-cs")],
                ..Default::default()
            };
            apply_signal_changes(&state, &tx, &notifier, signals, ts(secs), &mut prevs).await;
        }

        assert!(
            state.ev_sessions().await.is_empty(),
            "a VTN state-of-charge command must not create an EV session"
        );
        let notes = state.notifications_since(None).await;
        assert_eq!(notes.len(), 1, "announced once, not once per poll");
        assert_eq!(notes[0].severity, UserNotificationSeverity::Info);
        assert_eq!(notes[0].event_id.as_deref(), Some("evt-cs"));
        assert!(notes[0].message.contains("CHARGE_STATE_SETPOINT"));
        assert!(notes[0].message.contains("not applied"));
        assert!(
            notes[0].message.contains("driver"),
            "the reason travels with it"
        );
    }

    #[tokio::test]
    async fn a_second_event_with_an_unapplied_payload_is_announced_too() {
        let state = AppState::new();
        let (tx, _rx) = tokio::sync::watch::channel(PlanTriggerSignal::bare(PlanTrigger::Periodic));
        let mut prevs = SignalPrevs::default();
        let notifier = crate::services::notify::Notifier::new(None);

        let first = ParsedSignals {
            unapplied: vec![unapplied("evt-1")],
            ..Default::default()
        };
        apply_signal_changes(&state, &tx, &notifier, first, ts(0), &mut prevs).await;
        let both = ParsedSignals {
            unapplied: vec![unapplied("evt-1"), unapplied("evt-2")],
            ..Default::default()
        };
        apply_signal_changes(&state, &tx, &notifier, both, ts(30), &mut prevs).await;

        let ids: Vec<_> = state
            .notifications_since(None)
            .await
            .iter()
            .filter_map(|n| n.event_id.clone())
            .collect();
        assert_eq!(ids, vec!["evt-1", "evt-2"]);
    }

    #[tokio::test]
    async fn test_alert_appearance_emits_one_grid_emergency_notification() {
        use crate::entities::capacity::AlertWindow;
        use crate::entities::design_vocabulary::UserNotificationSeverity;
        let state = AppState::new();
        let (tx, _rx) = tokio::sync::watch::channel(PlanTriggerSignal::bare(PlanTrigger::Periodic));
        let notifier = crate::services::notify::Notifier::new(None);
        let mut prevs = SignalPrevs::default();

        let alert = AlertWindow {
            alert_type: "GRID_EMERGENCY".to_string(),
            start: ts(0),
            end: ts(3600),
            event_id: "evt-a".to_string(),
            message: "shed all load".to_string(),
        };
        let signals = ParsedSignals {
            alerts: vec![alert.clone()],
            ..Default::default()
        };
        apply_signal_changes(&state, &tx, &notifier, signals, ts(0), &mut prevs).await;

        // Same alert set on the next poll -> no duplicate notification.
        let signals = ParsedSignals {
            alerts: vec![alert],
            ..Default::default()
        };
        apply_signal_changes(&state, &tx, &notifier, signals, ts(30), &mut prevs).await;

        let notes = state.notifications_since(None).await;
        assert_eq!(notes.len(), 1, "exactly one notification per alert window");
        assert_eq!(notes[0].severity, UserNotificationSeverity::Alert);
        assert_eq!(notes[0].event_id.as_deref(), Some("evt-a"));
        assert!(notes[0].message.contains("shed all load"));
    }

    fn randomized_event(id: &str, window: Option<&str>) -> crate::controller::vtn_port::OadrEvent {
        let mut period = serde_json::json!({"start": "2026-03-21T11:00:00Z", "duration": "PT1H"});
        if let Some(w) = window {
            period["randomizeStart"] = serde_json::json!(w);
        }
        lab_core::test_fixtures::events_from_json(serde_json::json!([{
            "id": id, "programID": "p", "intervalPeriod": period,
            "intervals": [{"id": 0, "payloads": [{"type": "SIMPLE", "values": [1]}]}]
        }]))
        .remove(0)
    }

    #[test]
    fn staggered_starts_names_only_the_events_that_move() {
        let declared = vec![
            randomized_event("moves", Some("PT10M")),
            randomized_event("plain", None),
        ];
        let acted_on = declared
            .iter()
            .map(|e| lab_core::event_timing::with_randomized_start(e, "ven-1"))
            .collect::<Vec<_>>();
        let moved = staggered_starts(&declared, &acted_on);
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].0, "moves");
        assert!(
            moved[0].1 > chrono::Duration::zero() && moved[0].1 < chrono::Duration::minutes(10)
        );
    }

    #[tokio::test]
    async fn a_staggered_start_is_announced_once_per_event() {
        let state = AppState::new();
        let notifier = crate::services::notify::Notifier::new(None);
        let declared = vec![randomized_event("moves", Some("PT10M"))];
        let acted_on = declared
            .iter()
            .map(|e| lab_core::event_timing::with_randomized_start(e, "ven-1"))
            .collect::<Vec<_>>();
        let mut announced = std::collections::HashSet::new();

        for secs in [0, 30] {
            announce_staggered_starts(
                &state,
                &notifier,
                &declared,
                &acted_on,
                ts(secs),
                &mut announced,
            )
            .await;
        }

        let notes = state.notifications_since(None).await;
        assert_eq!(notes.len(), 1, "once, not once per poll");
        assert_eq!(notes[0].event_id.as_deref(), Some("moves"));
        assert!(notes[0].message.contains("randomizeStart"));
    }
}
