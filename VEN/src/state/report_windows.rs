//! What each report has already told the VTN (D-3).
//!
//! The accumulation rule itself lives in `controller::report_accumulator`;
//! this is only where the remembered window is kept, keyed by the report and
//! resource it belongs to. Two resources of the same report are two series and
//! must not be merged into one another.
//!
//! In memory only, on purpose — see that module's note on what a restart
//! costs.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use super::AppState;
use crate::controller::report_accumulator::{accumulate, is_stale, MAX_REPORT_INTERVALS};
use crate::controller::vtn_port::OadrReportInterval;

/// One report-resource's accumulated intervals, and when they were last
/// touched — which is what lets a window for a finished event be dropped
/// rather than kept for the life of the process.
#[derive(Debug, Clone, Default)]
pub struct ReportWindow {
    pub intervals: Vec<OadrReportInterval>,
    pub last_used: Option<DateTime<Utc>>,
}

pub type ReportWindows = HashMap<String, ReportWindow>;

impl AppState {
    /// Merge this submission's intervals into what the report already carried,
    /// and hand back the whole window to send.
    ///
    /// Called once per report resource per submission. Windows untouched for
    /// long enough are dropped in the same pass: an event that ended is not
    /// coming back, and its intervals should not outlive it in memory.
    pub async fn accumulate_report_intervals(
        &self,
        key: &str,
        fresh: Vec<OadrReportInterval>,
        now: DateTime<Utc>,
    ) -> Vec<OadrReportInterval> {
        let mut windows = self.report_windows.write().await;
        windows.retain(|k, w| k == key || w.last_used.is_none_or(|t| !is_stale(t, now)));

        let entry = windows.entry(key.to_string()).or_default();
        entry.intervals = accumulate(&entry.intervals, &fresh, MAX_REPORT_INTERVALS);
        entry.last_used = Some(now);
        entry.intervals.clone()
    }

    /// How many intervals each report is currently carrying, for diagnostics.
    pub async fn report_window_sizes(&self) -> Vec<(String, usize)> {
        self.report_windows
            .read()
            .await
            .iter()
            .map(|(k, w)| (k.clone(), w.intervals.len()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::vtn_port::{OadrIntervalPeriod, OadrReportPayload};

    fn interval(start: &str) -> OadrReportInterval {
        OadrReportInterval {
            id: 0,
            intervalPeriod: Some(OadrIntervalPeriod {
                start: Some(start.to_string()),
                duration: Some("PT1M".to_string()),
                randomizeStart: None,
            }),
            payloads: vec![OadrReportPayload::power_kw("DEMAND", 1.0)],
        }
    }

    fn t(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[tokio::test]
    async fn successive_submissions_build_one_growing_series() {
        let state = AppState::new();
        let first = state
            .accumulate_report_intervals(
                "r1",
                vec![interval("2026-09-22T10:00:00Z")],
                t("2026-09-22T10:01:00Z"),
            )
            .await;
        assert_eq!(first.len(), 1);

        let second = state
            .accumulate_report_intervals(
                "r1",
                vec![interval("2026-09-22T10:01:00Z")],
                t("2026-09-22T10:02:00Z"),
            )
            .await;
        assert_eq!(
            second.len(),
            2,
            "the second submission must carry the first interval too, or the \
             VTN's copy of the report shrinks back to one"
        );
    }

    /// Two reports are two series. Merging them would put one VEN's battery
    /// state into another report's power series.
    #[tokio::test]
    async fn windows_are_separate_per_report() {
        let state = AppState::new();
        state
            .accumulate_report_intervals(
                "r1",
                vec![interval("2026-09-22T10:00:00Z")],
                t("2026-09-22T10:01:00Z"),
            )
            .await;
        let other = state
            .accumulate_report_intervals(
                "r2",
                vec![interval("2026-09-22T10:00:00Z")],
                t("2026-09-22T10:01:00Z"),
            )
            .await;
        assert_eq!(other.len(), 1);
    }

    /// A window for an event that finished hours ago is dropped rather than
    /// carried for the life of the process.
    #[tokio::test]
    async fn a_window_untouched_for_hours_is_forgotten() {
        let state = AppState::new();
        state
            .accumulate_report_intervals(
                "old",
                vec![interval("2026-09-22T10:00:00Z")],
                t("2026-09-22T10:00:00Z"),
            )
            .await;
        state
            .accumulate_report_intervals(
                "current",
                vec![interval("2026-09-22T20:00:00Z")],
                t("2026-09-22T20:00:00Z"),
            )
            .await;
        let sizes = state.report_window_sizes().await;
        assert_eq!(sizes.len(), 1);
        assert_eq!(sizes[0].0, "current");
    }
}
