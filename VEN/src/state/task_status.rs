//! WP-T3 (`docs/history/project_journal.md, search "WP-T"`) accessors — split out of
//! `mod.rs` to keep it under the file-size cap; behaves as an ordinary
//! `impl AppState` block.
//!
//! Tracks `supervised_spawn`'s restart behavior per task name. Every
//! supervised task is, by design, an infinite loop that only returns to
//! `supervised_spawn`'s `await` when it panics — so `last_success` stays
//! `None` for the entire healthy lifetime of a task's current run, only
//! becoming `Some(false)`/`Some(true)` once that run actually completes.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::AppState;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskStatus {
    pub last_run_ts: Option<DateTime<Utc>>,
    pub last_success: Option<bool>,
    pub restart_count: u32,
}

impl AppState {
    /// Snapshot of every task that has been (re)spawned at least once.
    pub async fn task_statuses(&self) -> HashMap<String, TaskStatus> {
        self.task_status.read().await.clone()
    }

    /// Record that `name`'s wrapped future has just been (re)spawned.
    pub async fn record_task_started(&self, name: &str, now: DateTime<Utc>) {
        let mut guard = self.task_status.write().await;
        guard.entry(name.to_string()).or_default().last_run_ts = Some(now);
    }

    /// Record that `name`'s wrapped future just completed — `success: false`
    /// for a panic, `true` for the unusual case it returned `Ok(())`.
    pub async fn record_task_completed(&self, name: &str, success: bool) {
        let mut guard = self.task_status.write().await;
        let entry = guard.entry(name.to_string()).or_default();
        entry.last_success = Some(success);
        entry.restart_count += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn record_task_started_creates_entry_with_last_run_ts() {
        let state = AppState::new();
        let now = Utc::now();
        state.record_task_started("poll_events", now).await;

        let statuses = state.task_statuses().await;
        let entry = statuses.get("poll_events").expect("entry must exist");
        assert_eq!(entry.last_run_ts, Some(now));
        assert_eq!(entry.last_success, None);
        assert_eq!(entry.restart_count, 0);
    }

    #[tokio::test]
    async fn record_task_completed_sets_success_and_increments_restart_count() {
        let state = AppState::new();
        let now = Utc::now();
        state.record_task_started("sim_tick", now).await;
        state.record_task_completed("sim_tick", false).await;

        let statuses = state.task_statuses().await;
        let entry = statuses.get("sim_tick").expect("entry must exist");
        assert_eq!(entry.last_success, Some(false));
        assert_eq!(entry.restart_count, 1);

        state.record_task_started("sim_tick", Utc::now()).await;
        state.record_task_completed("sim_tick", true).await;
        let statuses = state.task_statuses().await;
        let entry = statuses.get("sim_tick").expect("entry must exist");
        assert_eq!(entry.last_success, Some(true));
        assert_eq!(entry.restart_count, 2);
    }

    #[tokio::test]
    async fn record_task_completed_before_started_still_creates_entry() {
        let state = AppState::new();
        state.record_task_completed("planning", false).await;

        let statuses = state.task_statuses().await;
        let entry = statuses.get("planning").expect("entry must exist");
        assert_eq!(entry.last_run_ts, None);
        assert_eq!(entry.last_success, Some(false));
        assert_eq!(entry.restart_count, 1);
    }
}
