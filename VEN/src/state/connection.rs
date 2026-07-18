//! WP-T1 (`docs/plans/ven-ui-transparency.md`) accessors — split out of `mod.rs`
//! to keep it under the file-size cap; behaves as an ordinary `impl AppState` block.
//!
//! `VtnConnectionStatus` tracks VTN reachability as observed by the `poll_events`
//! loop — the existing canonical outage-detection signal in this codebase (it
//! already drives `notify_outage_edge`). Process-lifetime only, not persisted;
//! `connected` defaults optimistic-until-first-poll, matching the poll loop's own
//! `let mut vtn_ok = true;` convention.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VtnConnectionStatus {
    pub connected: bool,
    pub last_success_ts: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub current_backoff_s: f64,
}

impl Default for VtnConnectionStatus {
    fn default() -> Self {
        Self {
            connected: true,
            last_success_ts: None,
            last_error: None,
            current_backoff_s: 0.0,
        }
    }
}

impl AppState {
    /// Current VTN reachability snapshot for `/health` and `/vtn/status`.
    pub async fn vtn_connection_status(&self) -> VtnConnectionStatus {
        self.vtn_connection.read().await.clone()
    }

    /// Record a successful VTN poll — clears any prior error and resets the
    /// backoff detail to zero (mirrors the poll loop's own reset-on-success).
    pub async fn record_vtn_poll_success(&self, now: DateTime<Utc>) {
        let mut guard = self.vtn_connection.write().await;
        guard.connected = true;
        guard.last_success_ts = Some(now);
        guard.last_error = None;
        guard.current_backoff_s = 0.0;
    }

    /// Record a failed VTN poll and the backoff delay before the next retry.
    pub async fn record_vtn_poll_failure(
        &self,
        _now: DateTime<Utc>,
        error: String,
        backoff_s: f64,
    ) {
        let mut guard = self.vtn_connection.write().await;
        guard.connected = false;
        guard.last_error = Some(error);
        guard.current_backoff_s = backoff_s;
    }

    /// Whether the last state-persist write succeeded.
    pub async fn storage_ok(&self) -> bool {
        *self.storage_ok.read().await
    }

    /// Record the outcome of a state-persist write attempt.
    pub async fn set_storage_ok(&self, ok: bool) {
        *self.storage_ok.write().await = ok;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn record_vtn_poll_success_clears_error_and_sets_connected() {
        let state = AppState::new();
        let now = Utc::now();
        state
            .record_vtn_poll_failure(now, "boom".to_string(), 30.0)
            .await;
        state.record_vtn_poll_success(now).await;

        let status = state.vtn_connection_status().await;
        assert!(status.connected);
        assert_eq!(status.last_success_ts, Some(now));
        assert_eq!(status.last_error, None);
        assert_eq!(status.current_backoff_s, 0.0);
    }

    #[tokio::test]
    async fn record_vtn_poll_failure_sets_error_and_backoff() {
        let state = AppState::new();
        let now = Utc::now();
        state
            .record_vtn_poll_failure(now, "connection refused".to_string(), 60.0)
            .await;

        let status = state.vtn_connection_status().await;
        assert!(!status.connected);
        assert_eq!(status.last_error, Some("connection refused".to_string()));
        assert_eq!(status.current_backoff_s, 60.0);
    }

    #[tokio::test]
    async fn storage_ok_defaults_true_and_reflects_last_write() {
        let state = AppState::new();
        assert!(state.storage_ok().await, "defaults optimistic");
        state.set_storage_ok(false).await;
        assert!(!state.storage_ok().await);
        state.set_storage_ok(true).await;
        assert!(state.storage_ok().await);
    }
}
