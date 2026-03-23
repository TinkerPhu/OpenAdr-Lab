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
    match sim.find_asset(&asset_id) {
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("unknown asset: {}", asset_id) })),
        )
            .into_response(),
        Some((entry, cfg)) => {
            let series = cfg.forecast(&entry.state, timespan);
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

/// GET /capability/:asset_id — point-in-time feasible power range for one asset (Phase A).
/// Returns `{"max_import_kw": ..., "max_export_kw": ..., "is_fixed": ...}`.
pub async fn get_asset_capability(
    State(ctx): State<AppCtx>,
    Path(asset_id): Path<String>,
) -> impl IntoResponse {
    use axum::http::StatusCode;

    let sim = ctx.sim.lock().await;
    match sim.find_asset(&asset_id) {
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("unknown asset: {}", asset_id) })),
        )
            .into_response(),
        Some((entry, cfg)) => {
            let cap = cfg.capability(&entry.state);
            Json(serde_json::json!({
                "max_import_kw": cap.max_import_kw,
                "max_export_kw": cap.max_export_kw,
                "is_fixed": cap.is_fixed(),
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
    let now = chrono::Utc::now();

    let sim = ctx.sim.lock().await;
    match sim.asset(&asset_id) {
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("unknown asset: {}", asset_id) })),
        )
            .into_response(),
        Some(entry) => {
            use crate::common::{Interpolation, TimeSeries};
            let points = entry.history.slice(timespan, now);
            // Prepend a LOCF boundary point at now-timespan so consumers always
            // get a sample anchored at the start of the requested window.
            let boundary_ts = now - timespan;
            let boundary_power = entry.history.power_at(boundary_ts).unwrap_or(0.0);
            let mut samples: Vec<(chrono::DateTime<chrono::Utc>, f64)> =
                vec![(boundary_ts, boundary_power)];
            samples.extend(points.iter().map(|p| (p.ts, p.power_kw)));
            let series = TimeSeries {
                samples,
                interpolation: Interpolation::Linear,
            };
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
