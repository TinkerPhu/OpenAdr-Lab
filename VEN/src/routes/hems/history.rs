//! WP1.4 — read routes over the persistent `HistoryPort` store (Phase 1, A-1).
//! Distinct from `/history/:asset_id` (routes/assets.rs), which serves the
//! live in-memory ring buffer — these serve the SQLite-backed archive.
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::str::FromStr;

use crate::entities::history::ForecastLeadKind;
use crate::AppCtx;

/// Requests spanning more than this many days are rejected — bounds response size.
const MAX_RANGE_DAYS: i64 = 7;

#[derive(Deserialize)]
pub struct HistoryRangeParams {
    pub from: Option<String>,
    pub to: Option<String>,
    pub asset_id: Option<String>,
}

fn error(status: StatusCode, msg: impl Into<String>) -> axum::response::Response {
    (status, Json(serde_json::json!({ "error": msg.into() }))).into_response()
}

/// Resolve and validate the `[from, to)` window: `to` defaults to now, `from`
/// defaults to `to - MAX_RANGE_DAYS`; any explicit range over the cap, or with
/// `from >= to`, is rejected with 400. Error side is a cheap `(StatusCode,
/// String)` rather than a full `Response` — keeps this `Result` small enough
/// that clippy's `result_large_err` doesn't fire; callers build the actual
/// response via `error()`.
fn resolve_range(
    params: &HistoryRangeParams,
) -> Result<(DateTime<Utc>, DateTime<Utc>), (StatusCode, String)> {
    let to = match &params.to {
        Some(s) => s
            .parse::<DateTime<Utc>>()
            .map_err(|_| (StatusCode::BAD_REQUEST, format!("invalid `to`: {s}")))?,
        None => Utc::now(),
    };
    let from = match &params.from {
        Some(s) => s
            .parse::<DateTime<Utc>>()
            .map_err(|_| (StatusCode::BAD_REQUEST, format!("invalid `from`: {s}")))?,
        None => to - chrono::Duration::days(MAX_RANGE_DAYS),
    };
    if from >= to {
        return Err((
            StatusCode::BAD_REQUEST,
            "`from` must be before `to`".to_string(),
        ));
    }
    if to - from > chrono::Duration::days(MAX_RANGE_DAYS) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("range exceeds the {MAX_RANGE_DAYS}-day cap"),
        ));
    }
    Ok((from, to))
}

macro_rules! history_range_route {
    ($fn_name:ident, $query:ident) => {
        pub async fn $fn_name(
            State(ctx): State<AppCtx>,
            Query(params): Query<HistoryRangeParams>,
        ) -> impl IntoResponse {
            let Some(history) = ctx.history.clone() else {
                return error(StatusCode::SERVICE_UNAVAILABLE, "history store disabled");
            };
            let (from, to) = match resolve_range(&params) {
                Ok(r) => r,
                Err((status, msg)) => return error(status, msg),
            };
            match tokio::task::spawn_blocking(move || history.$query(from, to)).await {
                Ok(Ok(rows)) => Json(rows).into_response(),
                Ok(Err(e)) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
                Err(e) => error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("task panicked: {e}"),
                ),
            }
        }
    };
}

// GET /history/grid?from=&to= — 1-minute grid samples in `[from, to)`.
history_range_route!(get_history_grid, query_grid);
// GET /history/events?from=&to= — OpenADR events received in `[from, to)`.
history_range_route!(get_history_events, query_events);
// GET /history/reports?from=&to= — OpenADR reports sent in `[from, to)`.
history_range_route!(get_history_reports, query_reports);

/// GET /history/ticks?from=&to=&asset_id= — 1-minute per-asset samples in
/// `[from, to)`, optionally filtered to one asset.
pub async fn get_history_ticks(
    State(ctx): State<AppCtx>,
    Query(params): Query<HistoryRangeParams>,
) -> impl IntoResponse {
    let Some(history) = ctx.history.clone() else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "history store disabled");
    };
    let (from, to) = match resolve_range(&params) {
        Ok(r) => r,
        Err((status, msg)) => return error(status, msg),
    };
    let asset_id = params.asset_id.clone();
    let result =
        tokio::task::spawn_blocking(move || history.query_ticks(from, to, asset_id.as_deref()))
            .await;
    match result {
        Ok(Ok(rows)) => Json(rows).into_response(),
        Ok(Err(e)) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        Err(e) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("task panicked: {e}"),
        ),
    }
}

#[derive(Deserialize)]
pub struct ForecastAccuracyParams {
    pub from: Option<String>,
    pub to: Option<String>,
    pub asset_id: Option<String>,
    pub lead_kind: Option<String>,
}

/// GET /history/forecast-accuracy?from=&to=&asset_id=&lead_kind= — near/far forecast samples
/// (predicted, and actual once reconciled) in `[from, to)`, optionally filtered to one asset
/// and/or lead kind. See `openspec/changes/forecast-accuracy-tracking/`.
pub async fn get_history_forecast_accuracy(
    State(ctx): State<AppCtx>,
    Query(params): Query<ForecastAccuracyParams>,
) -> impl IntoResponse {
    let Some(history) = ctx.history.clone() else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "history store disabled");
    };
    let range_params = HistoryRangeParams {
        from: params.from.clone(),
        to: params.to.clone(),
        asset_id: None,
    };
    let (from, to) = match resolve_range(&range_params) {
        Ok(r) => r,
        Err((status, msg)) => return error(status, msg),
    };
    let lead_kind = match params.lead_kind.as_deref().map(ForecastLeadKind::from_str) {
        Some(Ok(k)) => Some(k),
        Some(Err(msg)) => return error(StatusCode::BAD_REQUEST, msg),
        None => None,
    };
    let asset_id = params.asset_id.clone();
    let result = tokio::task::spawn_blocking(move || {
        history.query_forecast_accuracy(from, to, asset_id.as_deref(), lead_kind)
    })
    .await;
    match result {
        Ok(Ok(rows)) => Json(rows).into_response(),
        Ok(Err(e)) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        Err(e) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("task panicked: {e}"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(secs: i64) -> DateTime<Utc> {
        use chrono::TimeZone;
        Utc.timestamp_opt(secs, 0).unwrap()
    }

    #[test]
    fn test_resolve_range_defaults_to_last_max_range_days() {
        let params = HistoryRangeParams {
            from: None,
            to: None,
            asset_id: None,
        };
        let (from, to) = resolve_range(&params).unwrap();
        let span = to - from;
        assert_eq!(span, chrono::Duration::days(MAX_RANGE_DAYS));
    }

    #[test]
    fn test_resolve_range_explicit_window_within_cap_is_ok() {
        let params = HistoryRangeParams {
            from: Some(ts(0).to_rfc3339()),
            to: Some(ts(3600).to_rfc3339()),
            asset_id: None,
        };
        let (from, to) = resolve_range(&params).unwrap();
        assert_eq!(from, ts(0));
        assert_eq!(to, ts(3600));
    }

    #[test]
    fn test_resolve_range_rejects_from_after_to() {
        let params = HistoryRangeParams {
            from: Some(ts(3600).to_rfc3339()),
            to: Some(ts(0).to_rfc3339()),
            asset_id: None,
        };
        assert!(resolve_range(&params).is_err());
    }

    #[test]
    fn test_resolve_range_rejects_span_over_cap() {
        let params = HistoryRangeParams {
            from: Some(ts(0).to_rfc3339()),
            to: Some((ts(0) + chrono::Duration::days(MAX_RANGE_DAYS + 1)).to_rfc3339()),
            asset_id: None,
        };
        assert!(resolve_range(&params).is_err());
    }

    #[test]
    fn test_resolve_range_rejects_unparseable_timestamp() {
        let params = HistoryRangeParams {
            from: Some("not-a-date".to_string()),
            to: None,
            asset_id: None,
        };
        assert!(resolve_range(&params).is_err());
    }
}
