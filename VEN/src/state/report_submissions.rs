//! WP-T5 (G-5) — report submission outcomes, a `BoundedLog` rather than a
//! `HashMap` keyed by report identity: repeated submissions under the same
//! `report_name` are expected and each attempt is individually meaningful.
//! Split out of `mod.rs` to keep it under the file-size cap; behaves as an
//! ordinary `impl AppState` block.

use crate::entities::report_submission::ReportSubmissionRecord;

use super::AppState;

/// Cap chosen to comfortably exceed a normal session's submission volume
/// without growing unbounded (mirrors `NOTIFICATION_RING_CAP`'s rationale).
pub const REPORT_SUBMISSION_RING_CAP: usize = 100;

impl AppState {
    /// Append a submission outcome, evicting the oldest entry past the cap.
    pub async fn record_report_submission(&self, record: ReportSubmissionRecord) {
        self.report_submissions.record(record).await;
    }

    /// All recorded submission outcomes, newest first.
    pub async fn report_submissions(&self) -> Vec<ReportSubmissionRecord> {
        self.report_submissions.newest_first().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn record(name: &str) -> ReportSubmissionRecord {
        ReportSubmissionRecord::accepted(
            Some(name.into()),
            None,
            "ven-1",
            Utc.timestamp_opt(1_700_000_000, 0).unwrap(),
        )
    }

    #[tokio::test]
    async fn report_submissions_returns_newest_first() {
        let state = AppState::new();
        state.record_report_submission(record("r1")).await;
        state.record_report_submission(record("r2")).await;
        let all = state.report_submissions().await;
        assert_eq!(
            all.iter()
                .map(|r| r.report_name.clone().unwrap())
                .collect::<Vec<_>>(),
            vec!["r2", "r1"]
        );
    }

    // Eviction past the cap is `BoundedLog`'s own property, tested once in
    // `state/bounded_log.rs` rather than re-asserted per feed.
}
