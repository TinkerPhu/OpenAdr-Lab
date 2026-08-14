//! BL-43 — bounded in-memory ring of `SiteFlexibilityEnvelope` snapshots so
//! the UI can plot the live site-headroom band over time. Mirrors the
//! report-submission ring pattern (`state/report_submissions.rs`); unlike
//! `plan_history`/`forecast_accuracy` this is NOT persisted to SQLite — it's
//! a live diagnostic like `event_log`/`notifications`, not meant for
//! post-restart analysis.

use crate::entities::plan::{SiteFlexibilityEnvelope, SiteFlexibilitySample};

use super::AppState;

/// 1 hour of history at the dispatcher's ~1s tick cadence (see `GET /flexibility`'s
/// own doc comment) — matches the 1-hour default window every other Controller
/// chart already uses.
pub const FLEXIBILITY_HISTORY_RING_CAP: usize = 3600;

impl AppState {
    /// Append a sample, evicting the oldest entry past the cap.
    pub async fn record_flexibility_sample(&self, env: &SiteFlexibilityEnvelope) {
        self.flexibility_history.write().await.push(env.into());
    }

    /// All recorded samples, oldest first (time-series order) — the one
    /// difference from `report_submissions()`'s newest-first log-view convention.
    pub async fn flexibility_history(&self) -> Vec<SiteFlexibilitySample> {
        self.flexibility_history
            .read()
            .await
            .iter()
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn env(up_kw: f64, ts_s: i64) -> SiteFlexibilityEnvelope {
        SiteFlexibilityEnvelope {
            ts: Utc.timestamp_opt(ts_s, 0).unwrap(),
            up_kw,
            down_kw: 0.0,
            up_duration_s: None,
            down_duration_s: None,
        }
    }

    #[tokio::test]
    async fn flexibility_history_returns_oldest_first() {
        let state = AppState::new();
        state.record_flexibility_sample(&env(1.0, 100)).await;
        state.record_flexibility_sample(&env(2.0, 200)).await;
        let all = state.flexibility_history().await;
        assert_eq!(
            all.iter().map(|s| s.up_kw).collect::<Vec<_>>(),
            vec![1.0, 2.0]
        );
    }

    #[tokio::test]
    async fn ring_evicts_oldest_past_cap() {
        let state = AppState::new();
        for i in 0..(FLEXIBILITY_HISTORY_RING_CAP + 5) {
            state
                .record_flexibility_sample(&env(i as f64, i as i64))
                .await;
        }
        let all = state.flexibility_history().await;
        assert_eq!(all.len(), FLEXIBILITY_HISTORY_RING_CAP);
        assert_eq!(all.first().unwrap().up_kw, 5.0);
        assert_eq!(
            all.last().unwrap().up_kw,
            (FLEXIBILITY_HISTORY_RING_CAP + 4) as f64
        );
    }
}
