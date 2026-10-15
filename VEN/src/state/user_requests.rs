//! `AppState` accessors for the user requests and their lifecycle (R-137). Split out of
//! `state/mod.rs` like `ev_sessions.rs`.
//!
//! Owns how a request ends: cancelled by the user, displaced by a newer one, or completed when
//! its session's time has passed (`settle_requests`), each stamped through `UserRequest::finish`.
//! Reads the device sessions beside it under the same lock, so a request and the session it owns
//! never disagree. There is deliberately no whole-list setter: a tick that read the list, changed
//! it and wrote it back lost every request submitted in between.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::entities::asset_ledger::AssetLedgerEntry;
use crate::entities::device_session::HeaterTarget;
use crate::entities::user_request::{prune_finished, SessionType, UserRequest, UserRequestStatus};

use super::AppState;

impl AppState {
    /// Every listed request: the active ones and the finished ones still kept as history
    /// (`prune_finished`).
    pub async fn user_requests(&self) -> Vec<UserRequest> {
        self.hems.read().await.user_requests.clone()
    }

    /// Add a user request; replace if same id already exists.
    pub async fn upsert_request(&self, req: UserRequest) {
        let mut hems = self.hems.write().await;
        if let Some(pos) = hems.user_requests.iter().position(|r| r.id == req.id) {
            hems.user_requests[pos] = req;
        } else {
            hems.user_requests.push(req);
        }
    }

    /// Cancel a user request by id: marks it Cancelled and clears any linked device session.
    pub async fn cancel_request(&self, id: uuid::Uuid, now: DateTime<Utc>) -> bool {
        let mut hems = self.hems.write().await;
        if let Some(req) = hems.user_requests.iter_mut().find(|r| r.id == id) {
            req.finish(UserRequestStatus::Cancelled, now);
            let session_type = req.session_type.clone();
            let session_id = req.session_id;
            match session_type {
                Some(SessionType::Ev) => {
                    // By id, like the shiftable-load branch below: cancelling one
                    // request must not clear a session another request owns. The
                    // single slot made that impossible to express, so this is a
                    // correctness fix, not just a migration.
                    // A session id is unique across every EV's queue (W4), so only the queue
                    // that holds it changes.
                    if let Some(sid) = session_id {
                        for queue in hems.ev_sessions.values_mut() {
                            queue.remove(sid);
                        }
                    }
                }
                Some(SessionType::Heater) => {
                    // By id too: a request displaced by a newer heater request no longer
                    // owns the target, and cancelling it must not clear the newer one's.
                    hems.heater_targets.retain(|_, t| Some(t.id) != session_id);
                }
                Some(SessionType::ShiftableLoad) => {
                    if let Some(sid) = session_id {
                        hems.shiftable_loads.retain(|l| l.id != sid);
                    }
                }
                _ => {
                    tracing::warn!(request_id = %id, "cancel_request: unexpected session_type {:?} for request {}", session_type, id);
                }
            }
            true
        } else {
            false
        }
    }

    /// Set a heater's target (the heater `target.asset_id` names). A heater has one target, so
    /// the request that owned its previous one is cancelled with it, in the same critical section (the pairing
    /// `replace_ev_sessions` makes for a displaced EV session). Returns that request.
    pub async fn replace_heater_target(
        &self,
        target: HeaterTarget,
        now: DateTime<Utc>,
    ) -> Option<UserRequest> {
        let mut hems = self.hems.write().await;
        let displaced_id = hems
            .heater_targets
            .insert(target.asset_id.clone(), target)
            .map(|t| t.id);
        let req = hems
            .user_requests
            .iter_mut()
            .find(|r| r.session_id == displaced_id && !r.is_finished())?;
        req.finish(UserRequestStatus::Cancelled, now);
        Some(req.clone())
    }

    /// Close out a shiftable-load request once its asset has finished running
    /// (detected by `tasks::sim_tick::publish` as its asset_id disappearing
    /// from the live `SimSnapshot` — see `SimState::tick()`'s `is_removable`
    /// pass, design.md D3a of `shiftable-load-as-asset`).
    /// Returns the request it completed, if one was still active.
    pub async fn complete_shiftable(
        &self,
        load_id: uuid::Uuid,
        now: DateTime<Utc>,
    ) -> Option<UserRequest> {
        let mut w = self.hems.write().await;
        w.shiftable_loads.retain(|l| l.id != load_id);
        // Also mark linked UserRequest as Completed.
        let req = w
            .user_requests
            .iter_mut()
            .find(|r| r.session_id == Some(load_id) && !r.is_finished())?;
        req.finish(UserRequestStatus::Completed, now);
        Some(req.clone())
    }

    /// End what time has ended, once per tick: an EV session whose departure has passed and a
    /// heater target whose ready-by has passed are dropped, the requests that owned them are
    /// completed, and finished requests past their retention are pruned. Nothing else expires a
    /// session, so a passed one would otherwise act as an obligation, and list its request as
    /// active and overdue, forever. Returns the requests it completed.
    pub async fn settle_requests(&self, now: DateTime<Utc>) -> Vec<UserRequest> {
        let mut hems = self.hems.write().await;
        let mut ended: Vec<uuid::Uuid> = hems
            .ev_sessions
            .values_mut()
            .flat_map(|q| q.expire(now))
            .map(|s| s.id)
            .collect();
        hems.heater_targets.retain(|_, t| {
            let passed = t.ready_by <= now;
            if passed {
                ended.push(t.id);
            }
            !passed
        });
        let mut completed = Vec::new();
        for req in hems.user_requests.iter_mut() {
            if !req.is_finished() && req.session_id.is_some_and(|sid| ended.contains(&sid)) {
                req.finish(UserRequestStatus::Completed, now);
                completed.push(req.clone());
            }
        }
        prune_finished(&mut hems.user_requests, now);
        completed
    }

    /// Run the tick's ledger accounting over the ledger and the requests in place, under one
    /// lock (`controller::monitor::record_tick` is the caller's `account`).
    pub async fn account_tick(
        &self,
        account: impl FnOnce(&mut HashMap<String, AssetLedgerEntry>, &mut [UserRequest]),
    ) {
        let mut hems = self.hems.write().await;
        let hems = &mut *hems;
        account(&mut hems.asset_ledger, &mut hems.user_requests);
    }
}
