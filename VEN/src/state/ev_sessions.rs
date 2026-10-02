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

use crate::entities::device_session::{EvSession, EvSessionConflict, EvSessionQueue};

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
    pub async fn insert_ev_session(&self, session: EvSession) -> Result<(), EvSessionConflict> {
        self.hems.write().await.ev_sessions.insert(session)
    }

    /// Remove one session by id, returning it when it was queued.
    pub async fn remove_ev_session(&self, id: uuid::Uuid) -> Option<EvSession> {
        self.hems.write().await.ev_sessions.remove(id)
    }

    /// Drop every session whose departure has passed; returns how many went.
    pub async fn expire_ev_sessions(&self, now: DateTime<Utc>) -> usize {
        self.hems.write().await.ev_sessions.expire(now)
    }
}
