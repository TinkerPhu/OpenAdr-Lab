use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use chrono::Utc;
use serde::Deserialize;
use tracing::info;
use uuid::Uuid;

use crate::entities::asset::PlanTrigger;
use crate::entities::device_session::EvSession;
use crate::services::hems::EvSessionService;
use crate::AppCtx;

/// POST /ev-session body.
#[derive(Deserialize)]
pub struct CreateEvSessionBody {
    pub target_soc: f64,
    pub departure_time: chrono::DateTime<Utc>,
    /// If true, MILP treats charging as a soft reward (best-effort by departure).
    /// If false (default), charging is a hard constraint (must reach target SoC by departure).
    #[serde(default)]
    pub soft_deadline: bool,
    /// Request mode (BL-28); omitted = BY_DEADLINE (legacy behaviour).
    #[serde(default)]
    pub mode: crate::entities::design_vocabulary::UserRequestMode,
}

/// GET /ev-session — returns the active EV session (204 if none).
pub async fn get_ev_session(State(ctx): State<AppCtx>) -> impl IntoResponse {
    match ctx.state.ev_session().await {
        Some(s) => Json(s).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

/// POST /ev-session — create a new EV session, triggering a replan. Returns 409 if one already exists.
pub async fn post_ev_session(
    State(ctx): State<AppCtx>,
    Json(body): Json<CreateEvSessionBody>,
) -> impl IntoResponse {
    let now = Utc::now();
    let session = EvSession {
        id: Uuid::new_v4(),
        target_soc: body.target_soc,
        departure_time: body.departure_time,
        soft_deadline: body.soft_deadline,
        mode: body.mode,
        created_at: now,
        updated_at: now,
    };
    info!(
        session_id = %session.id,
        target_soc = session.target_soc,
        departure = %session.departure_time,
        "EV session created"
    );
    match EvSessionService::start(session.clone(), &ctx.state).await {
        Ok(()) => {
            let _ = ctx.trigger_tx.send(PlanTrigger::UserRequest);
            (StatusCode::CREATED, Json(session)).into_response()
        }
        Err(e) => {
            let (status, body) = e.into();
            (status, body).into_response()
        }
    }
}

/// DELETE /ev-session — clear the active EV session and complete any linked UserRequests.
pub async fn delete_ev_session(State(ctx): State<AppCtx>) -> impl IntoResponse {
    match EvSessionService::end(&ctx.state).await {
        Ok(()) | Err(crate::entities::error::DomainError::NotFound { .. }) => {
            let _ = ctx.trigger_tx.send(PlanTrigger::UserRequest);
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => {
            let (status, body) = e.into();
            (status, body).into_response()
        }
    }
}

/// GET /ev-settings — returns the current EV overlay settings.
pub async fn get_ev_settings(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.ev_settings().await)
}

/// PUT /ev-settings body.
#[derive(Deserialize)]
pub struct UpdateEvSettingsBody {
    pub opportunistic_charging_enabled: bool,
}

/// PUT /ev-settings — update the user toggle for opportunistic PV charging.
pub async fn put_ev_settings(
    State(ctx): State<AppCtx>,
    Json(body): Json<UpdateEvSettingsBody>,
) -> impl IntoResponse {
    let current = ctx.state.ev_settings().await;
    let updated = crate::state::EvSettings {
        opportunistic_charging_enabled: body.opportunistic_charging_enabled,
        paused_by_active_session: current.paused_by_active_session,
    };
    ctx.state.set_ev_settings(updated.clone()).await;
    Json(updated)
}
