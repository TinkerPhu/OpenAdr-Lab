use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    Json,
};
use serde::Deserialize;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

use crate::entities::asset::PlanTrigger;
use crate::entities::history::LedgerPeriod;
use crate::entities::PlannerObjective;
use crate::AppCtx;

/// GET /plan — returns the active Plan (null until Stage 3).
pub async fn get_plan(State(ctx): State<AppCtx>) -> impl IntoResponse {
    match ctx.state.active_plan().await {
        Some(plan) => Json(serde_json::to_value(plan).unwrap_or_default()).into_response(),
        None => Json(serde_json::Value::Null).into_response(),
    }
}

/// GET /forecast — per-asset forecasts from the latest plan cycle
/// (WP3.6, BL-15). Empty array until the first plan has been adopted.
pub async fn get_forecast(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.asset_forecasts().await)
}

/// PUT /plan/objective — change the active optimization objective and trigger an immediate replan.
#[derive(Debug, Deserialize)]
pub struct SetObjectiveBody {
    pub objective: PlannerObjective,
}

pub async fn put_plan_objective(
    State(ctx): State<AppCtx>,
    Json(body): Json<SetObjectiveBody>,
) -> impl IntoResponse {
    *ctx.active_objective.write().await = body.objective;
    let _ = ctx.trigger_tx.send(PlanTrigger::UserRequest);
    StatusCode::NO_CONTENT
}

/// GET /tariffs — returns planned tariff snapshots parsed from active events.
pub async fn get_tariffs(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.planned_tariffs().await)
}

/// GET /capacity — returns the current OadrCapacityState (Stage 2).
pub async fn get_capacity(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.capacity_state().await)
}

/// GET /signals — WP4.6: one-round-trip aggregate of the active grid signals
/// (alert / SIMPLE / dispatch windows + capacity state) for the UI status
/// strip. Read-only view over state the poll loop already maintains.
pub async fn get_signals(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(serde_json::json!({
        "alerts": ctx.state.alert_windows().await,
        "simple": ctx.state.simple_windows().await,
        "dispatch": ctx.state.dispatch_windows().await,
        "capacity": ctx.state.capacity_state().await,
    }))
}

/// GET /obligations — returns pending report obligations (Stage 2).
pub async fn get_obligations(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.report_obligations().await)
}

#[derive(Debug, Deserialize)]
pub struct LedgerQuery {
    pub asset_id: Option<String>,
}

/// GET /ledger — returns per-asset cumulative energy/cost/CO₂ for the current
/// open billing period (Stage 4). Unchanged shape when `asset_id` is absent
/// (existing Dashboard consumer). With `?asset_id=`, returns
/// `{ current, closed_periods }` for that one asset — `closed_periods` comes
/// from WP1.6's monthly `AssetLedger` rollover archive.
pub async fn get_ledger(
    State(ctx): State<AppCtx>,
    Query(params): Query<LedgerQuery>,
) -> impl IntoResponse {
    let current = ctx.state.asset_ledger().await;
    let Some(asset_id) = params.asset_id else {
        return Json(serde_json::to_value(&current).unwrap_or_default()).into_response();
    };

    let closed_periods: Vec<LedgerPeriod> = match ctx.history.clone() {
        Some(history) => {
            let aid = asset_id.clone();
            tokio::task::spawn_blocking(move || history.query_ledger_periods(&aid))
                .await
                .unwrap_or_else(|_| Ok(Vec::new()))
                .unwrap_or_default()
        }
        None => Vec::new(),
    };
    Json(serde_json::json!({
        "current": current.get(&asset_id),
        "closed_periods": closed_periods,
    }))
    .into_response()
}

/// GET /plan/events — Server-Sent Events stream of planner progress.
///
/// Pushes `solving_started`, `solving_progress` (1 s ticks), and `plan_ready`
/// events so the UI can show live solver feedback.
pub async fn get_plan_events(
    State(ctx): State<AppCtx>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let mut bcast_rx = ctx.planner_event_tx.subscribe();
    // Bridge broadcast → mpsc so lagged clients don't poison the broadcast sender.
    let (fwd_tx, fwd_rx) = tokio::sync::mpsc::channel::<Event>(32);
    tokio::spawn(async move {
        loop {
            match bcast_rx.recv().await {
                Ok(evt) => {
                    if let Ok(data) = serde_json::to_string(&evt) {
                        if fwd_tx.send(Event::default().data(data)).await.is_err() {
                            break; // client disconnected
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    let stream = ReceiverStream::new(fwd_rx).map(Ok::<_, std::convert::Infallible>);
    Sse::new(stream).keep_alive(KeepAlive::default())
}
