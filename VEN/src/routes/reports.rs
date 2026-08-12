use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use metrics::counter;
use tracing::error;

use crate::controller::vtn_port::OadrReportBody;
use crate::controller::HistoryPort;
use crate::entities::history::ReportSent;
use crate::entities::report_submission::ReportSubmissionRecord;
use crate::AppCtx;
use std::sync::Arc;

/// R-43 (design.md D3): append a `ReportSent` row, off the async runtime via
/// `spawn_blocking`, guarded on `history` being configured. No-op (not an
/// error) when history is disabled — matches every other optional-history
/// call site in this codebase (e.g. `tasks/poll_events`, `history_sampler`).
/// Takes `history` directly (not `&AppCtx`) so it stays unit-testable without
/// standing up a full `AppCtx` (whose `vtn: VtnClient` field is concrete,
/// not `dyn VtnPort` — out of this change's scope to retype, see design.md).
async fn record_report_sent(
    history: Option<Arc<dyn HistoryPort>>,
    report_type: Option<String>,
    event_id: Option<String>,
    now: chrono::DateTime<Utc>,
) {
    let Some(history) = history else {
        return;
    };
    let row = ReportSent {
        sent_at: now,
        report_type: report_type.unwrap_or_default(),
        event_id: event_id.unwrap_or_default(),
        payload_json: String::new(),
    };
    let _ = tokio::task::spawn_blocking(move || history.append_report_sent(&row)).await;
}

pub async fn get_reports(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.reports().await)
}

/// GET /reports/submissions — recent VEN-initiated submission outcomes
/// (WP-T5/G-5), newest first. Independent of `GET /reports`, which stays a
/// straight VTN-echo pass-through.
pub async fn get_report_submissions(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.report_submissions().await)
}

/// Build the submission-outcome record from an upsert/update result. Kept
/// pure (no I/O) so the accepted/rejected branching is unit-testable without
/// standing up a VTN HTTP stand-in.
fn submission_outcome(
    result: &anyhow::Result<()>,
    report_name: Option<String>,
    event_id: Option<String>,
    client_name: String,
    now: chrono::DateTime<Utc>,
) -> ReportSubmissionRecord {
    match result {
        Ok(()) => ReportSubmissionRecord::accepted(report_name, event_id, client_name, now),
        Err(e) => ReportSubmissionRecord::rejected(
            report_name,
            event_id,
            client_name,
            now,
            format!("{e:#}"),
        ),
    }
}

pub async fn post_reports(
    State(ctx): State<AppCtx>,
    Json(body): Json<OadrReportBody>,
) -> impl IntoResponse {
    let echo = body.clone();
    let (report_name, event_id, client_name) = (
        body.reportName.clone(),
        body.eventID.clone(),
        body.clientName.clone(),
    );
    let result = ctx.vtn.upsert_report(body).await;
    let now = Utc::now();
    ctx.state
        .record_report_submission(submission_outcome(
            &result,
            report_name.clone(),
            event_id.clone(),
            client_name,
            now,
        ))
        .await;
    match result {
        Ok(()) => {
            counter!("reports_sent_total").increment(1);
            record_report_sent(ctx.history.clone(), report_name, event_id, now).await;
            (axum::http::StatusCode::CREATED, Json(echo)).into_response()
        }
        Err(e) => {
            error!("report submission failed: {e:#}");
            (
                axum::http::StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": format!("{e:#}")})),
            )
                .into_response()
        }
    }
}

pub async fn put_report(
    State(ctx): State<AppCtx>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let report_name = body
        .get("reportName")
        .and_then(|v| v.as_str())
        .map(String::from);
    let event_id = body
        .get("eventID")
        .and_then(|v| v.as_str())
        .map(String::from);
    let client_name = body
        .get("clientName")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_default();
    match ctx.vtn.update_report(&id, body).await {
        Ok(result) => {
            counter!("reports_sent_total").increment(1);
            let now = Utc::now();
            ctx.state
                .record_report_submission(ReportSubmissionRecord::accepted(
                    report_name.clone(),
                    event_id.clone(),
                    client_name,
                    now,
                ))
                .await;
            record_report_sent(ctx.history.clone(), report_name, event_id, now).await;
            (axum::http::StatusCode::OK, Json(result)).into_response()
        }
        Err(e) => {
            error!("report update failed: {e:#}");
            ctx.state
                .record_report_submission(ReportSubmissionRecord::rejected(
                    report_name,
                    event_id,
                    client_name,
                    Utc::now(),
                    format!("{e:#}"),
                ))
                .await;
            (
                axum::http::StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": format!("{e:#}")})),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::test_support::mock_history_port::MockHistoryPort;
    use crate::state::AppState;

    fn now() -> chrono::DateTime<Utc> {
        Utc::now()
    }

    // ── R-43: record_report_sent ─────────────────────────────────────────

    #[tokio::test]
    async fn record_report_sent_appends_a_row_when_history_configured() {
        let history: Arc<dyn HistoryPort> = Arc::new(MockHistoryPort::new());
        let ts = now();
        record_report_sent(
            Some(history.clone()),
            Some("TELEMETRY_USAGE".to_string()),
            Some("evt-1".to_string()),
            ts,
        )
        .await;

        let rows = history
            .query_reports(
                ts - chrono::Duration::seconds(1),
                ts + chrono::Duration::seconds(1),
            )
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].report_type, "TELEMETRY_USAGE");
        assert_eq!(rows[0].event_id, "evt-1");
    }

    #[tokio::test]
    async fn record_report_sent_no_op_when_history_not_configured() {
        // Must not panic — this is the "no HistoryPort configured" scenario
        // shared across every R-43 call site.
        record_report_sent(
            None,
            Some("USAGE".to_string()),
            Some("evt-1".to_string()),
            now(),
        )
        .await;
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

        let failed = submission_outcome(
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
            .record_report_submission(submission_outcome(
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
