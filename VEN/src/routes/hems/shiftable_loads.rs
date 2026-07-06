use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use serde::Deserialize;
use tracing::{info, warn};
use uuid::Uuid;

use crate::entities::asset::PlanTrigger;
use crate::entities::device_session::ShiftableLoad;
use crate::AppCtx;

/// POST /shiftable-loads body.
#[derive(Deserialize)]
pub struct CreateShiftableLoadBody {
    pub asset_id: String,
    pub power_kw: f64,
    pub duration_min: u32,
    pub earliest_start: chrono::DateTime<Utc>,
    pub latest_end: chrono::DateTime<Utc>,
}

/// GET /shiftable-loads — returns all active shiftable loads.
pub async fn get_shiftable_loads(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.shiftable_loads().await)
}

/// POST /shiftable-loads — add a new shiftable load, triggering a replan.
pub async fn post_shiftable_load(
    State(ctx): State<AppCtx>,
    Json(body): Json<CreateShiftableLoadBody>,
) -> impl IntoResponse {
    let now = Utc::now();
    let load = ShiftableLoad {
        id: Uuid::new_v4(),
        asset_id: body.asset_id.clone(),
        power_kw: body.power_kw,
        duration_min: body.duration_min,
        earliest_start: body.earliest_start,
        latest_end: body.latest_end,
        created_at: now,
        updated_at: now,
    };
    info!(
        load_id = %load.id,
        asset_id = %load.asset_id,
        power_kw = load.power_kw,
        duration_min = load.duration_min,
        "shiftable load added"
    );
    match ctx.state.add_shiftable_load(load.clone()).await {
        Ok(()) => {
            let _ = ctx.trigger_tx.send(PlanTrigger::UserRequest);
            (StatusCode::CREATED, Json(load)).into_response()
        }
        Err(_) => {
            warn!(asset_id = %load.asset_id, "duplicate shiftable load asset_id");
            (
                StatusCode::CONFLICT,
                Json(serde_json::json!({"error": "duplicate asset_id"})),
            )
                .into_response()
        }
    }
}

/// DELETE /shiftable-loads/:id — remove a shiftable load by id.
pub async fn delete_shiftable_load(
    State(ctx): State<AppCtx>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    if ctx.state.remove_shiftable_load(id).await {
        let _ = ctx.trigger_tx.send(PlanTrigger::UserRequest);
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}
