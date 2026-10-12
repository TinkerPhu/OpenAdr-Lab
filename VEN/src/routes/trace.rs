use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use tracing::debug;

use crate::state::AppState;

#[derive(Deserialize)]
pub struct TraceQuery {
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
