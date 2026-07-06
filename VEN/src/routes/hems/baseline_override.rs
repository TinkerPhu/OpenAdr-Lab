use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use chrono::Utc;
use serde::Deserialize;
use tracing::info;
use uuid::Uuid;

use crate::entities::asset::PlanTrigger;
use crate::entities::device_session::{BaselineOverride, BaselineSlot};
use crate::AppCtx;

/// POST /baseline-override body.
#[derive(Deserialize)]
pub struct CreateBaselineOverrideBody {
    pub slots: Vec<BaselineSlotBody>,
}

#[derive(Deserialize)]
pub struct BaselineSlotBody {
    pub slot_start: chrono::DateTime<Utc>,
    pub add_kw: f64,
}

/// GET /baseline-override — returns the active baseline override (204 if none).
pub async fn get_baseline_override(State(ctx): State<AppCtx>) -> impl IntoResponse {
    match ctx.state.baseline_override().await {
        Some(o) => Json(o).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

/// POST /baseline-override — upsert the baseline override, triggering a replan.
pub async fn post_baseline_override(
    State(ctx): State<AppCtx>,
    Json(body): Json<CreateBaselineOverrideBody>,
) -> impl IntoResponse {
    let now = Utc::now();
    let ovr = BaselineOverride {
        id: Uuid::new_v4(),
        slots: body
            .slots
            .into_iter()
            .map(|s| BaselineSlot {
                slot_start: s.slot_start,
                add_kw: s.add_kw,
            })
            .collect(),
        created_at: now,
        updated_at: now,
    };
    info!(
        override_id = %ovr.id,
        slot_count = ovr.slots.len(),
        "baseline override set"
    );
    ctx.state.set_baseline_override(Some(ovr.clone())).await;
    let _ = ctx.trigger_tx.send(PlanTrigger::UserRequest);
    (StatusCode::CREATED, Json(ovr))
}

/// DELETE /baseline-override — clear the baseline override.
pub async fn delete_baseline_override(State(ctx): State<AppCtx>) -> impl IntoResponse {
    ctx.state.set_baseline_override(None).await;
    let _ = ctx.trigger_tx.send(PlanTrigger::UserRequest);
    StatusCode::NO_CONTENT
}
