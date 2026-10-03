//! `AppState` accessors for the EV's charging-session queue. Split out of
//! `state/mod.rs` once that file crossed the `VEN/src/` 500-production-line cap,
//! following the same precedent as `grid_signals.rs` and `obligations.rs` — an
//! `impl AppState` block may live in another file as long as the type does not.
//!
//! There is deliberately no `set_ev_sessions`: `EvSessionQueue::insert` is the only
//! way a session enters the queue, so no producer can write an overlapping queue
//! even by accident. See `EvSessionQueue` for why that invariant is enforced by
//! construction rather than by convention.

use chrono::{DateTime, Utc};

use crate::entities::device_session::{
    EvReplaceRefusal, EvSession, EvSessionClash, EvSessionQueue,
};

use super::AppState;

impl AppState {
    /// Every queued EV charging session, in window order.
    pub async fn ev_sessions(&self) -> EvSessionQueue {
        self.hems.read().await.ev_sessions.clone()
    }

    /// The session whose charging window contains `now`, if any.
    ///
    /// This is what callers asking "is a session active" want. Not "is the queue
    /// non-empty": under the rolling simulated schedule the queue is almost never
    /// empty, and conflating the two would pause opportunistic charging forever.
    pub async fn current_ev_session(&self, now: DateTime<Utc>) -> Option<EvSession> {
        self.hems.read().await.ev_sessions.current(now).cloned()
    }

    /// Queue a session, or report every queued session it overlaps.
    ///
    /// There is deliberately no `set_ev_sessions`: the checked insert is the only
    /// way in, so no producer can write an overlapping queue even by accident.
    pub async fn insert_ev_session(&self, session: EvSession) -> Result<(), EvSessionClash> {
        let mut hems = self.hems.write().await;
        match hems.ev_sessions.insert(session) {
            Ok(()) => Ok(()),
            // Resolved under the same guard that detected the clash, so what a
            // caller reports cannot have drifted from what was refused.
            Err(conflict) => Err(EvSessionClash {
                candidate: conflict.candidate,
                conflicts: conflict
                    .conflicts
                    .iter()
                    .filter_map(|id| hems.ev_sessions.iter().find(|s| s.id == *id).cloned())
                    .collect(),
            }),
        }
    }

    /// Remove one session by id, returning it when it was queued.
    pub async fn remove_ev_session(&self, id: uuid::Uuid) -> Option<EvSession> {
        self.hems.write().await.ev_sessions.remove(id)
    }

    /// Displace exactly the named sessions and queue `session`, atomically.
    ///
    /// One write critical section, so a concurrent submission cannot interleave
    /// between the removal and the insertion and leave the user with neither
    /// their old plan nor their new one. The enforcement is still
    /// `EvSessionQueue::replace`, which is still built on the checked `insert`:
    /// this method contributes the lock, not a second opinion about overlap.
    pub async fn replace_ev_sessions(
        &self,
        replace_ids: &[uuid::Uuid],
        session: EvSession,
    ) -> Result<Vec<EvSession>, EvReplaceRefusal> {
        let mut hems = self.hems.write().await;
        let clashing = hems.ev_sessions.conflicts(&session);
        match hems.ev_sessions.replace(replace_ids, session) {
            Ok(removed) => Ok(removed),
            // The conflict set is read under the same guard as the attempt, so a
            // re-prompt describes the queue the refusal was actually about.
            Err(rejection) => Err(EvReplaceRefusal {
                rejection,
                conflicts: clashing
                    .iter()
                    .filter_map(|id| hems.ev_sessions.iter().find(|s| s.id == *id).cloned())
                    .collect(),
            }),
        }
    }

    /// Drop every session whose departure has passed; returns how many went.
    pub async fn expire_ev_sessions(&self, now: DateTime<Utc>) -> usize {
        self.hems.write().await.ev_sessions.expire(now)
    }
}
