//! The report routes: what the VEN holds of its reports (`GET /reports`, submissions, windows)
//! and a report sent by hand (`POST /reports`, `PUT /reports/:id`). Reaches the VTN only
//! through `VtnPort`; never builds a report itself (`controller/reporter.rs` does).

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use metrics::counter;
use tracing::error;

use crate::app_ctx::{History, Vtn};
use crate::controller::history_port::record_report_sent;
use crate::controller::vtn_port::OadrReportBody;
use crate::entities::report_submission::ReportSubmissionRecord;
use crate::state::AppState;

pub async fn get_reports(State(state): State<AppState>) -> impl IntoResponse {
    Json(state.reports().await)
}

/// GET /reports/submissions — recent VEN-initiated submission outcomes
/// (WP-T5/G-5), newest first. Independent of `GET /reports`, which stays a
/// straight VTN-echo pass-through.
pub async fn get_report_submissions(State(state): State<AppState>) -> impl IntoResponse {
    Json(state.report_submissions().await)
}

/// One row of `GET /reports/windows`: how many intervals a report carries and its cap (D-3).
#[derive(Debug, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ReportWindowSize {
    pub report_name: String,
    pub intervals: usize,
    pub max_intervals: usize,
}

/// GET /reports/windows — how many intervals each report currently carries
/// (D-3), newest-named first.
///
/// The accumulated window decides what the VTN's copy of a report contains, so
/// it is state a reader must be able to see rather than infer from the reports
/// themselves (`ui-transparency`). A window stuck at one interval means
/// accumulation is not happening; one pinned at the cap means it is trimming.
pub async fn get_report_windows(State(state): State<AppState>) -> impl IntoResponse {
    let mut windows: Vec<ReportWindowSize> = state
        .report_window_sizes()
        .await
        .into_iter()
        .map(|(report_name, intervals)| ReportWindowSize {
            report_name,
            intervals,
            max_intervals: crate::controller::report_accumulator::MAX_REPORT_INTERVALS,
        })
        .collect();
    windows.sort_by(|a, b| a.report_name.cmp(&b.report_name));
    Json(windows)
}

/// Build the submission-outcome record from an upsert/update result. Kept
/// pure (no I/O) so the accepted/rejected branching is unit-testable without
/// standing up a VTN HTTP stand-in.
fn submission_outcome<T>(
    result: &anyhow::Result<T>,
    report_name: Option<String>,
    event_id: Option<String>,
    client_name: String,
    now: chrono::DateTime<Utc>,
) -> ReportSubmissionRecord {
    match result {
        Ok(_) => ReportSubmissionRecord::accepted(report_name, event_id, client_name, now),
        Err(e) => ReportSubmissionRecord::rejected(
            report_name,
            event_id,
            client_name,
            now,
            format!("{e:#}"),
        ),
    }
}

/// What a report body says about itself, kept for the outcome record once the body has gone
/// to the VTN.
struct ReportNames {
    report_name: Option<String>,
    event_id: Option<String>,
    client_name: String,
}

impl ReportNames {
    fn of(body: &OadrReportBody) -> Self {
        Self {
            report_name: body.reportName.clone(),
            event_id: body.eventID.clone(),
            client_name: body.clientName.clone(),
        }
    }
}

/// What both handlers do with the VTN's answer: record the outcome for
/// `GET /reports/submissions`, count and log an accepted report, and answer the caller with
/// `accepted_status` and the body, or with 502 and the VTN's reason.
async fn conclude<T: serde::Serialize>(
    state: &AppState,
    history: History,
    names: ReportNames,
    result: anyhow::Result<T>,
    accepted_status: StatusCode,
) -> Response {
    let now = Utc::now();
    state
        .record_report_submission(submission_outcome(
            &result,
            names.report_name.clone(),
            names.event_id.clone(),
            names.client_name,
            now,
        ))
        .await;
    match result {
        Ok(body) => {
            counter!("reports_sent_total").increment(1);
            record_report_sent(
                history,
                names.report_name.unwrap_or_default(),
                names.event_id.unwrap_or_default(),
                now,
            )
            .await;
            (accepted_status, Json(body)).into_response()
        }
        Err(e) => {
            error!("the VTN refused the report: {e:#}");
            crate::routes::error::error_response(StatusCode::BAD_GATEWAY, format!("{e:#}"))
        }
    }
}

pub async fn post_reports(
    State(vtn): State<Vtn>,
    State(state): State<AppState>,
    State(history): State<History>,
    Json(body): Json<OadrReportBody>,
) -> Response {
    let names = ReportNames::of(&body);
    let result = vtn.upsert_report(body.clone()).await.map(|()| body);
    conclude(&state, history, names, result, StatusCode::CREATED).await
}

pub async fn put_report(
    State(vtn): State<Vtn>,
    State(state): State<AppState>,
    State(history): State<History>,
    Path(id): Path<String>,
    Json(body): Json<OadrReportBody>,
) -> Response {
    let names = ReportNames::of(&body);
    let result = vtn.update_report(&id, body).await;
    conclude(&state, history, names, result, StatusCode::OK).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::HistoryPort;
    use crate::services::test_support::mock_history_port::MockHistoryPort;
    use crate::services::test_support::mock_vtn::MockVtn;
    use crate::state::AppState;
    use std::sync::Arc;

    fn now() -> chrono::DateTime<Utc> {
        Utc::now()
    }

    fn body(report_name: &str) -> OadrReportBody {
        OadrReportBody {
            eventID: Some("evt-1".into()),
            clientName: "ven-1".into(),
            reportName: Some(report_name.into()),
            resources: vec![],
            payloadDescriptors: vec![],
        }
    }

    async fn json_of(resp: Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// The one report the history holds, as `(report_type, event_id)`.
    fn sent_report(history: &MockHistoryPort) -> (String, String) {
        let t = now();
        let page = history
            .query_reports(
                t - chrono::Duration::minutes(1),
                t + chrono::Duration::minutes(1),
                100,
                0,
            )
            .unwrap();
        assert_eq!(page.rows.len(), 1);
        (
            page.rows[0].report_type.clone(),
            page.rows[0].event_id.clone(),
        )
    }

    #[tokio::test]
    async fn post_reports_accepted_returns_201_echo_and_records_submission() {
        let mock = Arc::new(MockVtn::new());
        let history = Arc::new(MockHistoryPort::new());
        let state = AppState::new();

        let resp = post_reports(
            State(mock.clone() as Vtn),
            State(state.clone()),
            State(Some(history.clone() as Arc<dyn HistoryPort>)),
            Json(body("r-1")),
        )
        .await;

        assert_eq!(resp.status(), StatusCode::CREATED);
        let echo = json_of(resp).await;
        assert_eq!(echo["reportName"], "r-1");
        assert_eq!(echo["eventID"], "evt-1");
        assert_eq!(mock.submitted().len(), 1);
        let subs = state.report_submissions().await;
        assert_eq!(subs.len(), 1);
        assert!(subs[0].vtn_accepted);
        assert_eq!(subs[0].report_name.as_deref(), Some("r-1"));
        assert_eq!(sent_report(&history), ("r-1".into(), "evt-1".into()));
    }

    #[tokio::test]
    async fn post_reports_rejected_returns_502_and_records_rejection() {
        let vtn: Vtn = Arc::new(MockVtn::new().with_upsert_error("event not found"));
        let state = AppState::new();

        let resp = post_reports(
            State(vtn),
            State(state.clone()),
            State(None),
            Json(body("r-1")),
        )
        .await;

        assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
        assert!(json_of(resp).await.to_string().contains("event not found"));
        let subs = state.report_submissions().await;
        assert_eq!(subs.len(), 1);
        assert!(!subs[0].vtn_accepted);
        assert!(subs[0]
            .error
            .as_deref()
            .unwrap()
            .contains("event not found"));
    }

    #[tokio::test]
    async fn put_report_accepted_returns_200_and_records_submission() {
        let mock = Arc::new(MockVtn::new());
        let history = Arc::new(MockHistoryPort::new());
        let state = AppState::new();

        let resp = put_report(
            State(mock.clone() as Vtn),
            State(state.clone()),
            State(Some(history.clone() as Arc<dyn HistoryPort>)),
            Path("rep-7".to_string()),
            Json(body("r-1")),
        )
        .await;

        assert_eq!(resp.status(), StatusCode::OK);
        let report = json_of(resp).await;
        assert_eq!(report["id"], "rep-7");
        assert_eq!(report["reportName"], "r-1");
        let updated = mock.updated();
        assert_eq!(updated.len(), 1);
        assert_eq!(updated[0].0, "rep-7");
        let subs = state.report_submissions().await;
        assert!(subs[0].vtn_accepted);
        assert_eq!(subs[0].client_name, "ven-1");
        assert_eq!(sent_report(&history), ("r-1".into(), "evt-1".into()));
    }

    #[tokio::test]
    async fn put_report_rejected_returns_502_and_records_rejection() {
        let vtn: Vtn = Arc::new(MockVtn::new().with_update_error("no such report"));
        let state = AppState::new();

        let resp = put_report(
            State(vtn),
            State(state.clone()),
            State(None),
            Path("rep-7".to_string()),
            Json(body("r-1")),
        )
        .await;

        assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
        assert!(json_of(resp).await.to_string().contains("no such report"));
        let subs = state.report_submissions().await;
        assert_eq!(subs.len(), 1);
        assert!(!subs[0].vtn_accepted);
    }

    #[tokio::test]
    async fn record_report_sent_appends_a_row_when_history_configured() {
        let history: Arc<dyn HistoryPort> = Arc::new(MockHistoryPort::new());
        let ts = now();
        record_report_sent(
            Some(history.clone()),
            "TELEMETRY_USAGE".to_string(),
            "evt-1".to_string(),
            ts,
        )
        .await;

        let page = history
            .query_reports(
                ts - chrono::Duration::seconds(1),
                ts + chrono::Duration::seconds(1),
                100,
                0,
            )
            .unwrap();
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0].report_type, "TELEMETRY_USAGE");
        assert_eq!(page.rows[0].event_id, "evt-1");
    }

    #[tokio::test]
    async fn record_report_sent_no_op_when_history_not_configured() {
        // Must not panic — this is the "no HistoryPort configured" scenario
        // shared across every R-43 call site.
        record_report_sent(None, "USAGE".to_string(), "evt-1".to_string(), now()).await;
    }

    #[test]
    fn test_report_submission_marks_vtn_accepted_on_success_and_false_on_failure() {
        let ok = submission_outcome(
            &Ok(()),
            Some("r-ok".into()),
            Some("evt-1".into()),
            "ven-1".into(),
            now(),
        );
        assert!(ok.vtn_accepted);
        assert_eq!(ok.report_name.as_deref(), Some("r-ok"));
        assert_eq!(ok.error, None);

        let failed = submission_outcome::<()>(
            &Err(anyhow::anyhow!("vtn unreachable")),
            Some("r-fail".into()),
            Some("evt-2".into()),
            "ven-1".into(),
            now(),
        );
        assert!(!failed.vtn_accepted);
        assert_eq!(failed.report_name.as_deref(), Some("r-fail"));
        assert!(failed.error.as_deref().unwrap().contains("vtn unreachable"));
    }

    #[tokio::test]
    async fn recorded_submissions_are_queryable_via_state() {
        let state = AppState::new();
        state
            .record_report_submission(submission_outcome(
                &Ok(()),
                Some("r-ok".into()),
                None,
                "ven-1".into(),
                now(),
            ))
            .await;
        state
            .record_report_submission(submission_outcome::<()>(
                &Err(anyhow::anyhow!("boom")),
                Some("r-fail".into()),
                None,
                "ven-1".into(),
                now(),
            ))
            .await;

        let subs = state.report_submissions().await;
        assert_eq!(subs.len(), 2);
        assert!(subs
            .iter()
            .any(|s| s.vtn_accepted && s.report_name.as_deref() == Some("r-ok")));
        assert!(subs
            .iter()
            .any(|s| !s.vtn_accepted && s.report_name.as_deref() == Some("r-fail")));
    }
}

#[cfg(test)]
mod report_window_wire_tests {
    use super::ReportWindowSize;

    /// R-133: the typed row is the `json!()` it replaced, key for key.
    #[test]
    fn report_window_size_keeps_the_wire_it_replaced() {
        let row = ReportWindowSize {
            report_name: "TELEMETRY".into(),
            intervals: 7,
            max_intervals: 120,
        };
        assert_eq!(
            serde_json::to_value(&row).unwrap(),
            serde_json::json!({ "reportName": "TELEMETRY", "intervals": 7, "maxIntervals": 120 })
        );
    }
}
