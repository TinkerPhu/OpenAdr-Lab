use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use tracing::debug;

use crate::app_ctx::SimRead;
use crate::state::AppState;

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
    State(state): State<AppState>,
    Query(q): Query<TraceQuery>,
) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(50);
    let ct = state.controller_trace().await;
    let mut events = ct.events();
    let total = events.len();
    events.reverse();
    events.truncate(limit);
    let plan_cycle_count = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                crate::controller::trace::ControllerEvent::PlanCycle { .. }
            )
        })
        .count();
    debug!(
        total_events = total,
        returned = events.len(),
        plan_cycle_count,
        "GET /trace/events"
    );
    Json(events)
}

/// GET /trace/history?asset=<id>&limit=N — returns timeline rows for an asset.
pub async fn get_trace_history(
    State(sim_read): State<SimRead>,
    Query(q): Query<TraceHistoryQuery>,
) -> impl IntoResponse {
    use chrono::{Duration, Utc};
    let limit = q.limit.unwrap_or(100);
    let now = Utc::now();
    // Slice up to 24 h of history (buffer holds ~1 h at 1 s tick; 24 h is a safe ceiling).
    let window = Duration::hours(24);

    let mut json: Vec<serde_json::Value> = sim_read
        .asset_trace(&q.asset, window, now)
        .await
        .into_iter()
        .map(|row| {
            let mut m = serde_json::Map::new();
            m.insert("ts".to_string(), serde_json::json!(row.ts));
            for (k, v) in row.values {
                m.insert(k, serde_json::json!(v));
            }
            serde_json::Value::Object(m)
        })
        .collect();

    json.reverse();
    json.truncate(limit);
    Json(json)
}
