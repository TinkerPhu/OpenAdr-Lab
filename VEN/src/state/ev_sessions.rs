//! `AppState` accessors for each EV's charging-session queue (W4: one queue per EV, keyed by
//! the session's own asset id). Split out of
//! `state/mod.rs` once that file crossed the `VEN/src/` 500-production-line cap,
//! following the same precedent as `grid_signals.rs` and `obligations.rs` — an
//! `impl AppState` block may live in another file as long as the type does not.
//!
//! There is deliberately no `set_ev_sessions`: `EvSessionQueue::insert` is the only
//! way a session enters the queue, so no producer can write an overlapping queue
//! even by accident. See `EvSessionQueue` for why that invariant is enforced by
//! construction rather than by convention.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};

use crate::entities::device_session::{
    EvReplaceRefusal, EvSession, EvSessionClash, EvSessionQueue,
};

use super::AppState;

impl AppState {
    /// Every EV's queued sessions by asset id, each queue in window order.
    pub async fn ev_sessions(&self) -> BTreeMap<String, EvSessionQueue> {
        self.hems.read().await.ev_sessions.clone()
    }

    /// EV `asset_id`'s queued sessions, in window order (empty when it has none).
    pub async fn ev_sessions_of(&self, asset_id: &str) -> EvSessionQueue {
        self.hems
            .read()
            .await
            .ev_sessions
            .get(asset_id)
            .cloned()
            .unwrap_or_default()
    }

    /// EV `asset_id`'s session whose charging window contains `now`, if any.
    ///
    /// This is what callers asking "is a session active" want. Not "is the queue
    /// non-empty": under the rolling simulated schedule the queue is almost never
    /// empty, and conflating the two would pause opportunistic charging forever.
    pub async fn current_ev_session(
        &self,
        asset_id: &str,
        now: DateTime<Utc>,
    ) -> Option<EvSession> {
        self.hems
            .read()
            .await
            .ev_sessions
            .get(asset_id)
            .and_then(|q| q.current(now).cloned())
    }

    /// Queue a session, or report every queued session it overlaps.
    ///
    /// There is deliberately no `set_ev_sessions`: the checked insert is the only
    /// way in, so no producer can write an overlapping queue even by accident.
    pub async fn insert_ev_session(&self, session: EvSession) -> Result<(), EvSessionClash> {
        let mut hems = self.hems.write().await;
        let queue = hems
            .ev_sessions
            .entry(session.asset_id.clone())
            .or_default();
        match queue.insert(session) {
            Ok(()) => Ok(()),
            // Resolved under the same guard that detected the clash, so what a
            // caller reports cannot have drifted from what was refused.
            Err(conflict) => Err(EvSessionClash {
                candidate: conflict.candidate,
                conflicts: conflict
                    .conflicts
                    .iter()
                    .filter_map(|id| queue.iter().find(|s| s.id == *id).cloned())
                    .collect(),
            }),
        }
    }

    /// Remove one session by id from whichever EV's queue holds it, returning it when queued.
    pub async fn remove_ev_session(&self, id: uuid::Uuid) -> Option<EvSession> {
        self.hems
            .write()
            .await
            .ev_sessions
            .values_mut()
            .find_map(|q| q.remove(id))
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
        now: DateTime<Utc>,
    ) -> Result<Vec<EvSession>, EvReplaceRefusal> {
        let mut hems = self.hems.write().await;
        let hems = &mut *hems;
        let queue = hems
            .ev_sessions
            .entry(session.asset_id.clone())
            .or_default();
        let clashing = queue.conflicts(&session);
        match queue.replace(replace_ids, session) {
            Ok(removed) => {
                // A displaced session's request is cancelled with it, in the same
                // critical section. The user asked for that plan to go, and leaving
                // its request Active would leave a commitment in the list pointing at
                // a session that no longer exists - which `GET /user-requests` would
                // then render as a request with no session at all. Same pairing
                // `cancel_request` makes from the other direction.
                for s in &removed {
                    if let Some(req) = hems
                        .user_requests
                        .iter_mut()
                        .find(|r| r.session_id == Some(s.id))
                    {
                        req.finish(
                            crate::entities::user_request::UserRequestStatus::Cancelled,
                            now,
                        );
                    }
                }
                Ok(removed)
            }
            // The conflict set is read under the same guard as the attempt, so a
            // re-prompt describes the queue the refusal was actually about.
            Err(rejection) => Err(EvReplaceRefusal {
                rejection,
                conflicts: clashing
                    .iter()
                    .filter_map(|id| queue.iter().find(|s| s.id == *id).cloned())
                    .collect(),
            }),
        }
    }
}
