//! Application of parsed grid-signal changes from one event poll (split out
//! of `poll_events.rs` for the tasks/ file-size cap): alert windows (WP3.1),
//! SIMPLE levels (WP3.2), and direct setpoints (WP3.4). Pure change-vs-prev
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
    pub charge_state: Option<(f64, DateTime<Utc>, String)>,
}

/// Previous-poll signal state, owned by the poll loop across iterations.
#[derive(Default)]
pub(crate) struct SignalPrevs {
    pub alerts: Vec<AlertWindow>,
    pub simple: Vec<SimpleWindow>,
    pub dispatch: Vec<DispatchWindow>,
    /// The EvSession id created by a CHARGE_STATE_SETPOINT event, so its
    /// disappearance (event deleted == cancelled) can clear that session —
    /// and only that one, never a user-created session.
    pub charge_state_session: Option<uuid::Uuid>,
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
        // Parsed but ignored while the VTN session path is disabled (below).
        charge_state: _charge_state,
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

    // WP3.4 DISABLED (2026-10-03): a VTN CHARGE_STATE_SETPOINT no longer creates
    // an EvSession. The code that did is preserved verbatim in
    // `apply_vtn_charge_state_session` below and is deliberately not called - see
    // that function's doc comment for why. `charge_state` is therefore parsed and
    // ignored, which is why it is bound with a leading underscore above.

    alerts_changed || simple_changed
}


/// WP3.4's VTN charge-state path: turn a `CHARGE_STATE_SETPOINT` into an
/// `EvSession`, and withdraw that session when the signal disappears.
///
/// **Not called, deliberately, and not to be wired up again without a decision.**
/// A VTN SoC command and a user's own charging request are different things: one
/// is an external constraint, the other is intent about the user's own car and
/// their own travel. Making both an `EvSession` distinguished only by `origin`
/// treats a grid preference as if it were the driver's plan.
///
/// The EV session queue made the cost concrete rather than theoretical: sessions
/// may not overlap, so a VTN session and a user session now compete for the same
/// calendar - a grid signal can be refused because the user has booked their car,
/// and worse, a VTN session can block the user from booking it at all. With the
/// old single slot this never surfaced, because the last writer simply won.
///
/// Kept rather than deleted because the parsing, the ownership-by-id bookkeeping
/// and the withdrawal semantics are all still correct and would have to be
/// rewritten identically if a future change decides a VTN SoC command belongs in
/// the planner as a *constraint* weighed against the user's sessions. That
/// decision is recorded as debt; it is not this function's to make.
#[allow(dead_code)] // disabled on purpose; see the doc comment above
async fn apply_vtn_charge_state_session(
    state: &AppState,
    trigger_tx: &tokio::sync::watch::Sender<PlanTriggerSignal>,
    charge_state: Option<(f64, DateTime<Utc>, String)>,
    now: DateTime<Utc>,
    prevs: &mut SignalPrevs,
) -> bool {
    // same state the user-request machinery uses. When the signal disappears
    // (event deleted == cancelled in OpenADR 3), the session it created is
    // cleared — user-created sessions are never touched.
    let mut session_changed = false;
    match charge_state {
        Some((target_soc, window_end, _eid)) => {
            // Only ever this producer's OWN session, identified by the id it
            // recorded. It used to read whatever sat in the single slot, so a VTN
            // signal whose target merely differed would overwrite a *user's*
            // session - a bug the slot made easy and the queue makes avoidable.
            let own = match prevs.charge_state_session {
                Some(id) => state.ev_sessions().await.iter().find(|s| s.id == id).cloned(),
                None => None,
            };
            let differs = own.as_ref().is_none_or(|s| {
                (s.target_soc - target_soc).abs() > 1e-9 || s.departure_time != window_end
            });
            if differs {
                // Retire its own previous session before stating the new one, so
                // the two cannot overlap each other.
                if let Some(prev) = prevs.charge_state_session.take() {
                    state.remove_ev_session(prev).await;
                }
                let id = uuid::Uuid::new_v4();
                let session = crate::entities::device_session::EvSession {
                    id,
                    target_soc,
                    // The signal commands a target by a window end; the vehicle is
                    // chargeable from now, which is what the single slot implied.
                    window_start: now,
                    departure_time: window_end,
                    soft_deadline: false,
                    // VTN-commanded charge target with a window end == a deadline.
                    mode: crate::entities::design_vocabulary::UserRequestMode::ByDeadline,
                    origin: crate::entities::device_session::EvSessionOrigin::Vtn,
                    budget_eur: None,
                    comfort_rates: vec![],
                    created_at: now,
                    updated_at: now,
                };
                match state.insert_ev_session(session).await {
                    Ok(()) => {
                        prevs.charge_state_session = Some(id);
                        let _ = trigger_tx.send(PlanTriggerSignal::bare(PlanTrigger::UserRequest));
                        session_changed = true;
                    }
                    Err(conflict) => {
                        // A stated session already covers this window. The signal
                        // is not dropped silently: it is reported, and the next
                        // poll retries once that session has passed.
                        tracing::warn!(
                            conflicts = ?conflict.conflicts,
                            %window_end,
                            "VTN charge-state signal overlaps an existing EV session; not queued"
                        );
                    }
                }
            }
        }
        None => {
            // The signal disappeared (deleted == cancelled in OpenADR 3): remove the
            // session this producer created, by id, and nothing else.
            if let Some(created_id) = prevs.charge_state_session.take() {
                if state.remove_ev_session(created_id).await.is_some() {
                    let _ = trigger_tx.send(PlanTriggerSignal::bare(PlanTrigger::UserRequest));
                    session_changed = true;
                }
            }
        }
    }
    session_changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn ts(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000 + secs, 0).unwrap()
    }

    /// The disabling itself, pinned: a charge-state signal reaching the live path
    /// must leave the session queue empty. Without this, re-wiring the VTN session
    /// path would be a silent change that every other test still passed.
    #[tokio::test]
    async fn a_charge_state_signal_creates_no_ev_session() {
        let state = AppState::new();
        let (tx, _rx) = tokio::sync::watch::channel(PlanTriggerSignal::bare(PlanTrigger::Periodic));
        let mut prevs = SignalPrevs::default();
        let notifier = crate::services::notify::Notifier::new(None);
        let signals = ParsedSignals {
            charge_state: Some((0.9, ts(7200), "evt-cs".to_string())),
            ..Default::default()
        };

        apply_signal_changes(&state, &tx, &notifier, signals, ts(0), &mut prevs).await;

        assert!(
            state.ev_sessions().await.is_empty(),
            "a VTN charge-state signal must not create an EV session"
        );
        assert!(
            prevs.charge_state_session.is_none(),
            "and must record no session of its own"
        );
    }

    /// Drives `apply_vtn_charge_state_session` directly, because
    /// `apply_signal_changes` no longer calls it (see its doc comment). The test is
    /// kept pointed at the preserved function rather than deleted: it is what
    /// proves that code still works if a future change decides to use it.
    #[tokio::test]
    async fn vtn_charge_state_path_creates_then_withdraws_its_own_session() {
        let state = AppState::new();
        let (tx, _rx) = tokio::sync::watch::channel(PlanTriggerSignal::bare(PlanTrigger::Periodic));
        let mut prevs = SignalPrevs::default();

        // Signal present -> session created.
        let signals = ParsedSignals {
            charge_state: Some((0.9, ts(7200), "evt-cs".to_string())),
            ..Default::default()
        };
        let sent =
            apply_vtn_charge_state_session(&state, &tx, signals.charge_state, ts(0), &mut prevs)
                .await;
        assert!(sent);
        let sessions = state.ev_sessions().await;
        assert_eq!(sessions.len(), 1, "exactly one session queued");
        let session = sessions.iter().next().unwrap();
        assert!((session.target_soc - 0.9).abs() < 1e-9);

        // Signal gone (event deleted == cancelled) -> that session cleared.
        let sent = apply_vtn_charge_state_session(&state, &tx, None, ts(10), &mut prevs).await;
        assert!(sent);
        assert!(
            state.ev_sessions().await.is_empty(),
            "event-created session cleared on event deletion"
        );
    }

    /// Same reason as above: aimed at the preserved function.
    #[tokio::test]
    async fn vtn_charge_state_withdrawal_leaves_a_user_session_alone() {
        let state = AppState::new();
        let (tx, _rx) = tokio::sync::watch::channel(PlanTriggerSignal::bare(PlanTrigger::Periodic));
        let mut prevs = SignalPrevs::default();

        // Event-created session, then a USER replaces it with their own.
        let signals = ParsedSignals {
            charge_state: Some((0.9, ts(7200), "evt-cs".to_string())),
            ..Default::default()
        };
        apply_vtn_charge_state_session(&state, &tx, signals.charge_state, ts(0), &mut prevs).await;
        // The VTN's own session covers [ts(0), ts(7200)); the user's sits after it,
        // so both are queued at once. Under the single slot this test had to
        // *overwrite* the VTN session to express "a user session exists", which
        // could not distinguish "left alone" from "never looked at".
        let user_session = crate::entities::device_session::EvSession {
            id: uuid::Uuid::new_v4(),
            target_soc: 0.7,
            window_start: ts(7200),
            departure_time: ts(10800),
            soft_deadline: false,
            origin: crate::entities::device_session::EvSessionOrigin::UserRequest,
            budget_eur: None,
            comfort_rates: vec![],
            mode: Default::default(),
            created_at: ts(5),
            updated_at: ts(5),
        };
        state
            .insert_ev_session(user_session.clone())
            .await
            .expect("a window after the VTN's must queue alongside it");

        apply_vtn_charge_state_session(&state, &tx, None, ts(10), &mut prevs).await;
        // The VTN removed only what it created, by id: the user's session remains
        // and is now the only one queued.
        let left: Vec<_> = state.ev_sessions().await.iter().map(|s| s.id).collect();
        assert_eq!(left, vec![user_session.id], "user session untouched");
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
}
