use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use metrics::counter;
use tracing::error;

use crate::controller::vtn_port::OadrReportBody;
use crate::entities::report_submission::ReportSubmissionRecord;
use crate::AppCtx;

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
    ctx.state
        .record_report_submission(submission_outcome(
            &result,
            report_name,
            event_id,
            client_name,
            Utc::now(),
        ))
        .await;
    match result {
        Ok(()) => {
            counter!("reports_sent_total").increment(1);
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
            ctx.state
                .record_report_submission(ReportSubmissionRecord::accepted(
                    report_name,
                    event_id,
                    client_name,
                    Utc::now(),
                ))
                .await;
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
    use crate::state::AppState;

    fn now() -> chrono::DateTime<Utc> {
        Utc::now()
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
