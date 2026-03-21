use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;

use crate::AppCtx;

/// Query parameters for GET /forecast/:asset_id.
#[derive(Deserialize)]
pub struct ForecastParams {
    pub timespan_s: Option<f64>,
}

/// Query parameters for GET /history/:asset_id.
#[derive(Deserialize)]
pub struct HistoryParams {
    pub timespan_s: Option<f64>,
}

/// GET /forecast/:asset_id — forward-looking TimeSeries for one asset (speckit 007).
/// Returns `{"samples": [{"ts": "...", "value": ...}], "interpolation": "..."}`.
pub async fn get_asset_forecast(
    State(ctx): State<AppCtx>,
    Path(asset_id): Path<String>,
    Query(params): Query<ForecastParams>,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    use chrono::Duration;

    let timespan_s = params.timespan_s.unwrap_or(0.0);
    let timespan = Duration::milliseconds((timespan_s * 1000.0) as i64);

    let sim = ctx.sim.lock().await;
    let entry = sim.assets.iter().find(|e| e.id == asset_id);
    match entry {
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("unknown asset: {}", asset_id) })),
        )
            .into_response(),
        Some(entry) => {
            let series = entry.state.forecast(timespan);
            let samples: Vec<serde_json::Value> = series
                .samples
                .iter()
                .map(|(ts, v)| serde_json::json!({ "ts": ts, "value": v }))
                .collect();
            Json(serde_json::json!({
                "samples": samples,
                "interpolation": series.interpolation,
            }))
            .into_response()
        }
    }
}

/// GET /history/:asset_id — historical TimeSeries for one asset (speckit 007).
/// Returns `{"samples": [{"ts": "...", "value": ...}], "interpolation": "..."}`.
pub async fn get_asset_history(
    State(ctx): State<AppCtx>,
    Path(asset_id): Path<String>,
    Query(params): Query<HistoryParams>,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    use chrono::Duration;

    let timespan_s = params.timespan_s.unwrap_or(0.0);
    let timespan = Duration::milliseconds((timespan_s * 1000.0) as i64);

    let ct = ctx.state.controller_trace().await;
    let sim = ctx.sim.lock().await;
    let entry = sim.assets.iter().find(|e| e.id == asset_id);
    match entry {
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("unknown asset: {}", asset_id) })),
        )
            .into_response(),
        Some(entry) => {
            let history = ct.asset_history.get(&asset_id);
            let empty_buf = crate::controller::trace::AssetHistoryBuffer::new(0);
            let buf = history.unwrap_or(&empty_buf);
            let series = entry.state.history(timespan, buf);
            let samples: Vec<serde_json::Value> = series
                .samples
                .iter()
                .map(|(ts, v)| serde_json::json!({ "ts": ts, "value": v }))
                .collect();
            Json(serde_json::json!({
                "samples": samples,
                "interpolation": series.interpolation,
            }))
            .into_response()
        }
    }
}
