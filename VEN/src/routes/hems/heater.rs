use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use chrono::Utc;
use serde::Deserialize;
use tracing::info;
use uuid::Uuid;

use crate::entities::asset::PlanTrigger;
use crate::entities::device_session::HeaterTarget;
use crate::AppCtx;

/// POST /heater-target body.
#[derive(Deserialize)]
pub struct CreateHeaterTargetBody {
    pub target_temp_c: f64,
    pub ready_by: chrono::DateTime<Utc>,
    /// Request mode (BL-28); omitted = BY_DEADLINE (legacy behaviour).
    #[serde(default)]
    pub mode: crate::entities::design_vocabulary::UserRequestMode,
}

/// GET /heater-target — returns the active heater target (204 if none).
pub async fn get_heater_target(State(ctx): State<AppCtx>) -> impl IntoResponse {
    match ctx.state.heater_target().await {
        Some(t) => Json(t).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

/// POST /heater-target — create or replace the active heater target, triggering a replan.
pub async fn post_heater_target(
    State(ctx): State<AppCtx>,
    Json(body): Json<CreateHeaterTargetBody>,
) -> impl IntoResponse {
    let now = Utc::now();
    let target = HeaterTarget {
        id: Uuid::new_v4(),
        target_temp_c: body.target_temp_c,
        ready_by: body.ready_by,
        mode: body.mode,
        created_at: now,
        updated_at: now,
    };
    info!(
        target_id = %target.id,
        target_temp_c = target.target_temp_c,
        ready_by = %target.ready_by,
        "heater target created"
    );
    ctx.state.set_heater_target(Some(target.clone())).await;
    let _ = ctx.trigger_tx.send(PlanTrigger::UserRequest);
    (StatusCode::CREATED, Json(target))
}

/// DELETE /heater-target — clear the active heater target.
pub async fn delete_heater_target(State(ctx): State<AppCtx>) -> impl IntoResponse {
    ctx.state.set_heater_target(None).await;
    let _ = ctx.trigger_tx.send(PlanTrigger::UserRequest);
    StatusCode::NO_CONTENT
}
