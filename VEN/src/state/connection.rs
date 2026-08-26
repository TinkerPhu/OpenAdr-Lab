//! WP-T1 (`docs/history/project_journal.md, search "WP-T"`) accessors — split out of `mod.rs`
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

impl VtnConnectionStatus {
    /// R-59: true once the VTN has been continuously unreachable for at
    /// least `debounce_s` seconds — derived from existing fields, no new
    /// tracking needed. Avoids nuisance-tripping comms-loss curtailment on
    /// a single transient poll blip (`tasks/backoff.rs` already retries with
    /// exponential backoff). `last_success_ts: None` (never yet succeeded,
    /// e.g. cold start) is treated as "not yet debounced" — optimistic,
    /// matching `connected`'s own cold-start default — rather than as an
    /// instant comms-loss.
    pub fn comms_lost_for(&self, now: DateTime<Utc>, debounce_s: u64) -> bool {
        if self.connected {
            return false;
        }
        match self.last_success_ts {
            Some(ts) => (now - ts).num_seconds() >= debounce_s as i64,
            None => false,
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

    // ── comms_lost_for (R-59 debounce) ─────────────────────────────────────

    #[test]
    fn comms_lost_for_false_when_connected() {
        let status = VtnConnectionStatus {
            connected: true,
            last_success_ts: Some(Utc::now() - chrono::Duration::seconds(120)),
            last_error: None,
            current_backoff_s: 0.0,
        };
        assert!(!status.comms_lost_for(Utc::now(), 60));
    }

    #[test]
    fn comms_lost_for_false_when_disconnected_but_under_debounce() {
        let now = Utc::now();
        let status = VtnConnectionStatus {
            connected: false,
            last_success_ts: Some(now - chrono::Duration::seconds(10)),
            last_error: Some("boom".into()),
            current_backoff_s: 5.0,
        };
        assert!(!status.comms_lost_for(now, 60));
    }

    #[test]
    fn comms_lost_for_true_when_disconnected_past_debounce() {
        let now = Utc::now();
        let status = VtnConnectionStatus {
            connected: false,
            last_success_ts: Some(now - chrono::Duration::seconds(61)),
            last_error: Some("boom".into()),
            current_backoff_s: 60.0,
        };
        assert!(status.comms_lost_for(now, 60));
    }

    #[test]
    fn comms_lost_for_false_when_never_succeeded() {
        let status = VtnConnectionStatus {
            connected: false,
            last_success_ts: None,
            last_error: Some("boom".into()),
            current_backoff_s: 5.0,
        };
        assert!(!status.comms_lost_for(Utc::now(), 60));
    }
}
