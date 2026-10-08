//! Installs the simulated usage schedule's predicted charge sessions into the EV session
//! queue. The EV predicts them (`Asset::planned_usage_sessions`); this is the application
//! half: put what it predicted into `AppState`, idempotently, without ever displacing a
//! stated session.

use crate::entities::device_session::EvSession;
use crate::state::AppState;

/// Queue each of `planned` that is not already queued; returns how many were added.
///
/// Idempotent by construction: it runs every tick, and re-deriving a trip already queued
/// must change nothing. A session that clashes with a stated one is skipped, never forced -
/// the simulated schedule stands in for a user, so it yields, and the remaining trips are
/// still placed around the stated session.
pub async fn install_planned_sessions(state: &AppState, planned: Vec<EvSession>) -> usize {
    let queued = state.ev_sessions().await;
    let mut added = 0;
    for session in planned {
        if queued
            .iter()
            .any(|s| s.departure_time == session.departure_time)
        {
            continue;
        }
        let departure_time = session.departure_time;
        match state.insert_ev_session(session).await {
            Ok(()) => added += 1,
            Err(conflict) => tracing::debug!(
                leave_at = %departure_time,
                // Ids, not whole sessions: the clash carries the sessions so a prompt can
                // describe them, but a log line wants the identity, not the payload.
                conflicts = ?conflict.conflicts.iter().map(|s| s.id).collect::<Vec<_>>(),
                "simulated EV session skipped: a stated session covers this window"
            ),
        }
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::device_session::EvSessionOrigin;
    use chrono::{Duration, TimeZone, Utc};

    fn now() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 20, 6, 0, 0).unwrap()
    }

    fn simulated(from_h: i64, to_h: i64) -> EvSession {
        EvSession::simulated(
            0.8,
            now() + Duration::hours(from_h),
            now() + Duration::hours(to_h),
            now(),
        )
    }

    #[tokio::test]
    async fn install_planned_sessions_queues_every_session() {
        let state = AppState::new();
        let added =
            install_planned_sessions(&state, vec![simulated(0, 2), simulated(10, 12)]).await;
        assert_eq!(added, 2);
        let queued = state.ev_sessions().await;
        assert_eq!(queued.len(), 2);
        assert!(queued
            .iter()
            .all(|s| s.origin == EvSessionOrigin::SimulatedUsage));
    }

    /// Runs every tick, so re-deriving the same schedule must add nothing.
    #[tokio::test]
    async fn install_planned_sessions_is_idempotent() {
        let state = AppState::new();
        install_planned_sessions(&state, vec![simulated(0, 2), simulated(10, 12)]).await;
        let first: Vec<_> = state.ev_sessions().await.iter().map(|s| s.id).collect();
        let added =
            install_planned_sessions(&state, vec![simulated(0, 2), simulated(10, 12)]).await;
        let second: Vec<_> = state.ev_sessions().await.iter().map(|s| s.id).collect();
        assert_eq!(added, 0);
        assert_eq!(first, second, "a second pass must change nothing");
    }

    #[tokio::test]
    async fn install_planned_sessions_never_overwrites_a_stated_session() {
        let state = AppState::new();
        let mut stated = simulated(0, 3);
        stated.origin = EvSessionOrigin::UserRequest;
        state.insert_ev_session(stated.clone()).await.unwrap();

        // The first overlaps the stated session and yields; the second is placed around it.
        let added =
            install_planned_sessions(&state, vec![simulated(1, 2), simulated(10, 12)]).await;

        assert_eq!(added, 1);
        let queued = state.ev_sessions().await;
        assert!(
            queued.iter().any(|s| s.id == stated.id),
            "the stated session is untouched"
        );
        assert_eq!(queued.len(), 2);
    }
}
