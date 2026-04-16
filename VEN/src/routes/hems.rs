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

use crate::controller::user_request::CreateUserRequestBody;
use crate::entities::asset::PlanTrigger;
use crate::entities::device_session::{BaselineOverride, BaselineSlot, EvSession, HeaterTarget, ShiftableLoad};
use crate::AppCtx;

/// GET /plan — returns the active Plan (null until Stage 3).
pub async fn get_plan(
    State(ctx): State<AppCtx>,
) -> impl IntoResponse {
    match ctx.state.active_plan().await {
        Some(plan) => {
            Json(serde_json::to_value(plan).unwrap_or_default()).into_response()
        }
        None => Json(serde_json::Value::Null).into_response(),
    }
}

/// GET /tariffs — returns planned tariff snapshots parsed from active events.
pub async fn get_tariffs(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.planned_tariffs().await)
}

/// GET /capacity — returns the current OadrCapacityState (Stage 2).
pub async fn get_capacity(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.capacity_state().await)
}

/// GET /obligations — returns pending report obligations (Stage 2).
pub async fn get_obligations(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.report_obligations().await)
}

/// GET /ledger — returns per-asset cumulative energy/cost/CO₂ (Stage 4).
pub async fn get_ledger(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.asset_ledger().await)
}

/// GET /user-requests — list all user requests.
pub async fn get_requests(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.active_requests().await)
}

/// POST /user-requests — create a user energy task request (Stage 5).
pub async fn post_requests(
    State(ctx): State<AppCtx>,
    Json(body): Json<CreateUserRequestBody>,
) -> impl IntoResponse {
    let now = Utc::now();
    let (assets, asset_configs) = {
        let sim = ctx.sim.lock().await;
        (sim.assets.clone(), sim.asset_configs.clone())
    };

    match crate::controller::user_request::create_from_body(body, &assets, &asset_configs, now) {
        Ok(mut user_req) => {
            // Create the appropriate device session based on asset_id
            if user_req.asset_id == "ev" {
                let deadline = user_req
                    .deadlines
                    .first()
                    .map(|d| d.latest_end)
                    .unwrap_or_else(|| now + chrono::Duration::hours(8));
                let target_soc = user_req.target_soc.unwrap_or(0.9);
                let session = EvSession {
                    id: Uuid::new_v4(),
                    target_soc,
                    departure_time: deadline,
                    opportunistic: false,
                    created_at: now,
                    updated_at: now,
                };
                user_req.session_id = Some(session.id);
                ctx.state.set_ev_session(Some(session.clone())).await;
                info!(
                    request_id = %user_req.id,
                    session_id = %session.id,
                    asset_id = %user_req.asset_id,
                    target_soc,
                    "user request created (EV session)"
                );
            } else if user_req.asset_id == "heater" || user_req.asset_id == "boiler" {
                let ready_by = user_req
                    .deadlines
                    .first()
                    .map(|d| d.latest_end)
                    .unwrap_or_else(|| now + chrono::Duration::hours(4));
                let target_temp_c = 55.0_f64; // default heater target; use /hems/heater-target for custom temp
                let target = HeaterTarget {
                    id: Uuid::new_v4(),
                    target_temp_c,
                    ready_by,
                    created_at: now,
                    updated_at: now,
                };
                user_req.session_id = Some(target.id);
                ctx.state.set_heater_target(Some(target.clone())).await;
                info!(
                    request_id = %user_req.id,
                    session_id = %target.id,
                    asset_id = %user_req.asset_id,
                    target_temp_c,
                    "user request created (heater target)"
                );
            } else {
                info!(
                    request_id = %user_req.id,
                    asset_id = %user_req.asset_id,
                    "user request created"
                );
            }
            ctx.state.upsert_request(user_req.clone()).await;
            ctx.state
                .push_controller_event(
                    crate::controller::trace::ControllerEvent::RequestTransition {
                        ts: now,
                        request_id: user_req.id,
                        asset_id: user_req.asset_id.clone(),
                        from_status: "None".to_string(),
                        to_status: format!("{:?}", user_req.status),
                    },
                )
                .await;
            let _ = ctx.trigger_tx.send(PlanTrigger::UserRequest);
            (
                axum::http::StatusCode::CREATED,
                Json(serde_json::to_value(user_req).unwrap_or_default()),
            )
                .into_response()
        }
        Err(e) => {
            warn!("POST /user-requests rejected: {e}");
            (
                axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        }
    }
}

/// DELETE /user-requests/:id — cancel a user request and clear any linked device session.
pub async fn delete_request(State(ctx): State<AppCtx>, Path(id): Path<Uuid>) -> impl IntoResponse {
    if ctx.state.cancel_request(id).await {
        ctx.state
            .push_controller_event(
                crate::controller::trace::ControllerEvent::RequestTransition {
                    ts: Utc::now(),
                    request_id: id,
                    asset_id: String::new(),
                    from_status: "Active".to_string(),
                    to_status: "Cancelled".to_string(),
                },
            )
            .await;
        let _ = ctx.trigger_tx.send(PlanTrigger::UserRequest);
        info!(request_id = %id, "user request cancelled");
        axum::http::StatusCode::NO_CONTENT.into_response()
    } else {
        (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "request not found"})),
        )
            .into_response()
    }
}

/// GET /flexibility — returns the live site-level flexibility envelope (Phase E).
///
/// Updated every dispatcher tick (~1s) and after every planner cycle.
/// Returns 204 No Content until the first dispatcher tick completes.
pub async fn get_flexibility(State(ctx): State<AppCtx>) -> impl IntoResponse {
    match ctx.state.site_envelope().await {
        Some(env) => Json(env).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

// ── Device-centric session endpoints (Phase A) ──────────────────────────────

/// POST /ev-session body.
#[derive(Deserialize)]
pub struct CreateEvSessionBody {
    pub target_soc: f64,
    pub departure_time: chrono::DateTime<Utc>,
    #[serde(default)]
    pub opportunistic: bool,
}

/// GET /ev-session — returns the active EV session (204 if none).
pub async fn get_ev_session(State(ctx): State<AppCtx>) -> impl IntoResponse {
    match ctx.state.ev_session().await {
        Some(s) => Json(s).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

/// POST /ev-session — create or replace the active EV session, triggering a replan.
pub async fn post_ev_session(
    State(ctx): State<AppCtx>,
    Json(body): Json<CreateEvSessionBody>,
) -> impl IntoResponse {
    let now = Utc::now();
    let session = EvSession {
        id: Uuid::new_v4(),
        target_soc: body.target_soc,
        departure_time: body.departure_time,
        opportunistic: body.opportunistic,
        created_at: now,
        updated_at: now,
    };
    info!(
        session_id = %session.id,
        target_soc = session.target_soc,
        departure = %session.departure_time,
        "EV session created"
    );
    ctx.state.set_ev_session(Some(session.clone())).await;
    let _ = ctx.trigger_tx.send(PlanTrigger::UserRequest);
    (StatusCode::CREATED, Json(session))
}

/// DELETE /ev-session — clear the active EV session.
pub async fn delete_ev_session(State(ctx): State<AppCtx>) -> impl IntoResponse {
    ctx.state.set_ev_session(None).await;
    let _ = ctx.trigger_tx.send(PlanTrigger::UserRequest);
    StatusCode::NO_CONTENT
}

/// POST /heater-target body.
#[derive(Deserialize)]
pub struct CreateHeaterTargetBody {
    pub target_temp_c: f64,
    pub ready_by: chrono::DateTime<Utc>,
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

// ── Shiftable-load endpoints (Phase B) ──────────────────────────────────────

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
    ctx.state.add_shiftable_load(load.clone()).await;
    let _ = ctx.trigger_tx.send(PlanTrigger::UserRequest);
    (StatusCode::CREATED, Json(load))
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

// ── Baseline-override endpoints (Phase B) ───────────────────────────────────

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
