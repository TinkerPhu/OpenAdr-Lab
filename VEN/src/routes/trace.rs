use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;

use crate::AppCtx;

#[derive(Deserialize)]
pub struct TraceQuery {
    pub limit: Option<usize>,
}

#[derive(Deserialize)]
pub struct TraceHistoryQuery {
    pub asset: String,
    pub limit: Option<usize>,
}

/// GET /trace/events?limit=N — returns recent ControllerEvent log entries (newest first).
pub async fn get_trace_events(
    State(ctx): State<AppCtx>,
    Query(q): Query<TraceQuery>,
) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(50);
    let ct = ctx.state.controller_trace().await;
    let mut events = ct.events();
    events.reverse();
    events.truncate(limit);
    Json(events)
}

/// GET /trace/history?asset=<id>&limit=N — returns timeline rows for an asset.
pub async fn get_trace_history(
    State(ctx): State<AppCtx>,
    Query(q): Query<TraceHistoryQuery>,
) -> impl IntoResponse {
    let ct = ctx.state.controller_trace().await;
    let limit = q.limit.unwrap_or(100);
    match ct.asset_history_for(&q.asset) {
        None => Json(Vec::<serde_json::Value>::new()).into_response(),
        Some(buf) => {
            let mut points = buf.to_timeline(None);
            points.reverse();
            points.truncate(limit);
            let json: Vec<serde_json::Value> = points
                .into_iter()
                .map(|p| {
                    let mut m = serde_json::Map::new();
                    m.insert("ts".to_string(), serde_json::json!(p.ts));
                    for (k, v) in p.values {
                        m.insert(k, serde_json::json!(v));
                    }
                    serde_json::Value::Object(m)
                })
                .collect();
            Json(json).into_response()
        }
    }
}
