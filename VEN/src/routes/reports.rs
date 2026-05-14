use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use metrics::counter;
use tracing::error;

use crate::controller::vtn_port::OadrReportBody;
use crate::AppCtx;

pub async fn get_reports(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.reports().await)
}

pub async fn post_reports(
    State(ctx): State<AppCtx>,
    Json(body): Json<OadrReportBody>,
) -> impl IntoResponse {
    let echo = body.clone();
    match ctx.vtn.upsert_report(body).await {
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
    match ctx.vtn.update_report(&id, body).await {
        Ok(result) => {
            counter!("reports_sent_total").increment(1);
            (axum::http::StatusCode::OK, Json(result)).into_response()
        }
        Err(e) => {
            error!("report update failed: {e:#}");
            (
                axum::http::StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": format!("{e:#}")})),
            )
                .into_response()
        }
    }
}
