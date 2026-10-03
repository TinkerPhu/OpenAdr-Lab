//! The VTN charge-state path, preserved and **not wired up** (R-100).
//!
//! Split into its own file so the disabling is visible in the module list rather
//! than buried at the bottom of `poll_signals.rs` — and so that file stays under
//! the 200-production-line `tasks/` cap.

use chrono::{DateTime, Utc};

use super::poll_signals::SignalPrevs;
use crate::entities::asset::{PlanTrigger, PlanTriggerSignal};
use crate::state::AppState;

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
pub(crate) async fn apply_vtn_charge_state_session(
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
                Some(id) => state
                    .ev_sessions()
                    .await
                    .iter()
                    .find(|s| s.id == id)
                    .cloned(),
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
                    expected_trip_distance_km: None,
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
                            // Ids, not whole sessions: the clash carries the sessions so a prompt can
                    // describe them, but a log line wants the identity, not the payload.
                    conflicts = ?conflict.conflicts.iter().map(|s| s.id).collect::<Vec<_>>(),
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
