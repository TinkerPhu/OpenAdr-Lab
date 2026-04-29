use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use tracing::{debug, warn};

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
    let total = events.len();
    events.reverse();
    events.truncate(limit);
    let plan_cycle_count = events
        .iter()
        .filter(|e| matches!(e, crate::controller::trace::ControllerEvent::PlanCycle { .. }))
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
    State(ctx): State<AppCtx>,
    Query(q): Query<TraceHistoryQuery>,
) -> impl IntoResponse {
    use chrono::{Duration, Utc};
    let limit = q.limit.unwrap_or(100);
    let now = Utc::now();
    // Slice up to 24 h of history (buffer holds ~1 h at 1 s tick; 24 h is a safe ceiling).
    let window = Duration::hours(24);

    let lock_start = std::time::Instant::now();
    let sim = ctx.sim.lock().await;
    let lock_ms = lock_start.elapsed().as_millis();
    if lock_ms > 100 {
        warn!(
            lock_wait_ms = lock_ms,
            asset = %q.asset,
            "GET /trace/history: sim mutex wait was long (planner may be running)"
        );
    } else {
        debug!(lock_wait_ms = lock_ms, asset = %q.asset, "GET /trace/history: sim mutex acquired");
    }

    let mut json: Vec<serde_json::Value> = match sim.find_asset(&q.asset) {
        None => vec![],
        Some((entry, cfg)) => entry
            .history
            .slice(window, now)
            .into_iter()
            .map(|p| {
                let mut values = cfg.state_values(&p.state);
                values.insert("power_kw".into(), p.power_kw);
                let mut m = serde_json::Map::new();
                m.insert("ts".to_string(), serde_json::json!(p.ts));
                for (k, v) in values {
                    m.insert(k, serde_json::json!(v));
                }
                serde_json::Value::Object(m)
            })
            .collect(),
    };
    drop(sim);

    json.reverse();
    json.truncate(limit);
    Json(json)
}
