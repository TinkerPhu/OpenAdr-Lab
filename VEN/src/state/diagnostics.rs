// ── DiagnosticsState: the VEN's own health signals, grouped ───────────────────
// R-47's remaining half. `AppState` had four flat `Arc<RwLock<..>>` fields for
// what the VEN knows about itself — VTN reachability, refused wire objects, the
// last persist outcome, per-task restart status — and every observability work
// package added another. Grouping them means the next one is a field here, not a
// fifth entry on the application's root struct.
//
// This is the same move `bounded_log::BoundedLog<T>` already made for the four
// near-identical bounded rings: name the family, then let new members join it by
// construction instead of by remembering the pattern.
//
// Deliberately NOT one lock over the whole struct: these four are written by four
// independent tasks (`poll_events`, the wire-health notifier, `state_persist`,
// `supervised_spawn`), and a shared lock would couple them into one another's
// contention for no gain. The grouping is about where the fields live, not about
// making them atomic together.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tokio::sync::RwLock;

use super::connection::VtnConnectionStatus;
use super::task_status::TaskStatus;

#[derive(Clone)]
pub struct DiagnosticsState {
    /// WP-T1: VTN reachability, written by `tasks::poll_events`, read by
    /// `GET /health` and `GET /vtn/status`.
    pub vtn_connection: Arc<RwLock<VtnConnectionStatus>>,
    /// Objects the VTN sent that we refused, keyed by resource. See
    /// `state/wire_health.rs`.
    pub wire_rejections: Arc<RwLock<BTreeMap<String, String>>>,
    /// WP-T1: whether the last state-persist write succeeded, written by
    /// `tasks::state_persist`, read by `GET /health`.
    pub storage_ok: Arc<RwLock<bool>>,
    /// WP-T3: per-task restart/outcome status, written by
    /// `tasks::supervised_spawn`, read by `GET /tasks/status`. Keyed by task
    /// name; entries created lazily.
    pub task_status: Arc<RwLock<HashMap<String, TaskStatus>>>,
}

impl Default for DiagnosticsState {
    fn default() -> Self {
        Self::new()
    }
}

impl DiagnosticsState {
    /// `storage_ok` starts optimistic: nothing has failed yet, and reporting a
    /// storage fault before the first write was attempted would be a false alarm.
    pub fn new() -> Self {
        Self {
            vtn_connection: Arc::new(RwLock::new(VtnConnectionStatus::default())),
            wire_rejections: Arc::new(RwLock::new(BTreeMap::new())),
            storage_ok: Arc::new(RwLock::new(true)),
            task_status: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}
