//! Application of parsed grid-signal changes from one event poll (split out
//! of `poll_events.rs` for the tasks/ file-size cap): alert windows (WP3.1),
//! SIMPLE levels (WP3.2), and direct setpoints (WP3.4). Pure change-vs-prev
//! bookkeeping plus state writes and plan triggers — no parsing here.

use chrono::{DateTime, Utc};

use crate::controller;
use crate::entities::asset::PlanTrigger;
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
}

/// Apply this poll's parsed signals. Returns `true` when a plan trigger was
/// already sent (Alert / CapacityChange / UserRequest) — the caller must then
/// not overwrite it with RateChange, since `trigger_tx` is a watch channel
/// where only the latest value survives.
pub(crate) async fn apply_signal_changes(
    state: &AppState,
    trigger_tx: &tokio::sync::watch::Sender<PlanTrigger>,
    signals: ParsedSignals,
    now: DateTime<Utc>,
    prevs: &mut SignalPrevs,
) -> bool {
    let ParsedSignals {
        alerts,
        simple,
        dispatch,
        charge_state,
    } = signals;
    // WP3.1 (BL-04): alert changes replan with the Alert trigger.
    let alerts_changed = alerts != prevs.alerts;
    if alerts_changed {
        state.set_alert_windows(alerts.clone()).await;
        prevs.alerts = alerts;
        let _ = trigger_tx.send(PlanTrigger::Alert);
    }

    // WP3.2: SIMPLE changes replan as CapacityChange (they constrain the
    // same per-slot import cap); Alert wins the label if both changed.
    let simple_changed = simple != prevs.simple;
    if simple_changed {
        state.set_simple_windows(simple.clone()).await;
        prevs.simple = simple;
        if !alerts_changed {
            let _ = trigger_tx.send(PlanTrigger::CapacityChange);
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

    // WP3.4: CHARGE_STATE_SETPOINT creates/updates an EvSession through the
    // same state the user-request machinery uses.
    let mut session_changed = false;
    if let Some((target_soc, window_end, _eid)) = charge_state {
        let existing = state.ev_session().await;
        let differs = existing.as_ref().is_none_or(|s| {
            (s.target_soc - target_soc).abs() > 1e-9 || s.departure_time != window_end
        });
        if differs {
            state
                .set_ev_session(Some(crate::entities::device_session::EvSession {
                    id: uuid::Uuid::new_v4(),
                    target_soc,
                    departure_time: window_end,
                    soft_deadline: false,
                    created_at: now,
                    updated_at: now,
                }))
                .await;
            let _ = trigger_tx.send(PlanTrigger::UserRequest);
            session_changed = true;
        }
    }

    alerts_changed || simple_changed || session_changed
}
