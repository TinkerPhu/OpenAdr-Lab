//! WP-T4 (`docs/history/project_journal.md, search "WP-T"`) — Event Log: VEN-operational
//! events (VTN connectivity failures, storage errors, task panics/restarts),
//! deliberately separate from the resident-facing Notifications feed
//! (`services/notify.rs`) — different ring, different broadcast channel,
//! different route, no dedup, no persistence (see design.md for why).
//! Split out of `mod.rs` to keep it under the file-size cap.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;

use super::AppState;

pub const EVENT_LOG_RING_CAP: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventLogEntry {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub category: String,
    pub message: String,
}

impl AppState {
    /// Record a VEN-operational event. Every occurrence is recorded — no
    /// dedup — bounded only by evicting the oldest entry past the ring cap.
    pub async fn record_event(
        &self,
        now: DateTime<Utc>,
        category: &str,
        message: impl Into<String>,
    ) {
        let entry = EventLogEntry {
            id: Uuid::new_v4(),
            created_at: now,
            category: category.to_string(),
            message: message.into(),
        };
        {
            let mut ring = self.event_log.write().await;
            ring.push_back(entry.clone());
            while ring.len() > EVENT_LOG_RING_CAP {
                ring.pop_front();
            }
        }
        let _ = self.event_log_tx.send(entry);
    }

    /// Current ring contents, oldest first.
    pub async fn event_log_snapshot(&self) -> Vec<EventLogEntry> {
        self.event_log.read().await.iter().cloned().collect()
    }

    /// Subscribe for live updates (SSE bridge).
    pub fn subscribe_event_log(&self) -> broadcast::Receiver<EventLogEntry> {
        self.event_log_tx.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn record_event_appends_and_broadcasts() {
        let state = AppState::new();
        let mut rx = state.subscribe_event_log();
        let now = Utc::now();

        state.record_event(now, "storage", "write failed").await;

        let snapshot = state.event_log_snapshot().await;
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].category, "storage");
        assert_eq!(snapshot[0].message, "write failed");
        assert_eq!(snapshot[0].created_at, now);

        let broadcast_entry = rx.recv().await.expect("entry must be broadcast");
        assert_eq!(broadcast_entry.id, snapshot[0].id);
    }

    #[tokio::test]
    async fn record_event_evicts_oldest_beyond_ring_cap() {
        let state = AppState::new();
        let now = Utc::now();
        for i in 0..EVENT_LOG_RING_CAP + 5 {
            state
                .record_event(now, "task_supervisor", format!("entry {i}"))
                .await;
        }

        let snapshot = state.event_log_snapshot().await;
        assert_eq!(snapshot.len(), EVENT_LOG_RING_CAP, "ring stays at capacity");
        assert_eq!(
            snapshot[0].message, "entry 5",
            "the oldest 5 entries were evicted"
        );
    }

    #[tokio::test]
    async fn event_log_snapshot_returns_oldest_first() {
        let state = AppState::new();
        let now = Utc::now();
        state.record_event(now, "vtn_connection", "first").await;
        state.record_event(now, "vtn_connection", "second").await;

        let snapshot = state.event_log_snapshot().await;
        assert_eq!(snapshot[0].message, "first");
        assert_eq!(snapshot[1].message, "second");
    }
}
