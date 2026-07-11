use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use tracing::{info, warn};
use uuid::Uuid;

use super::{SessionDetail, UserRequestWithSession};
use crate::controller::user_request::CreateUserRequestBody;
use crate::entities::asset::PlanTrigger;
use crate::entities::asset_params::AssetRequestSlice;
use crate::entities::device_session::ShiftableLoad;
use crate::entities::user_request::{SessionType, UserRequest, UserRequestStatus};
use crate::services::user_request::UserRequestService;
use crate::AppCtx;

/// GET /user-requests — list all user requests with embedded session details.
pub async fn get_requests(State(ctx): State<AppCtx>) -> impl IntoResponse {
    let requests = ctx.state.active_requests().await;
    let ev = ctx.state.ev_session().await;
    let heater = ctx.state.heater_target().await;
    let loads = ctx.state.shiftable_loads().await;

    let enriched: Vec<UserRequestWithSession> = requests
        .into_iter()
        .map(|req| {
            let session = req.session_id.and_then(|sid| {
                match req.session_type {
                    Some(SessionType::Ev) => ev
                        .as_ref()
                        .filter(|s| s.id == sid)
                        .cloned()
                        .map(SessionDetail::Ev),
                    Some(SessionType::Heater) => heater
                        .as_ref()
                        .filter(|t| t.id == sid)
                        .cloned()
                        .map(SessionDetail::Heater),
                    Some(SessionType::ShiftableLoad) => loads
                        .iter()
                        .find(|l| l.id == sid)
                        .cloned()
                        .map(SessionDetail::ShiftableLoad),
                    None => {
                        // Legacy: try all session types by id match
                        if let Some(s) = ev.as_ref().filter(|s| s.id == sid) {
                            return Some(SessionDetail::Ev(s.clone()));
                        }
                        if let Some(t) = heater.as_ref().filter(|t| t.id == sid) {
                            return Some(SessionDetail::Heater(t.clone()));
                        }
                        loads
                            .iter()
                            .find(|l| l.id == sid)
                            .cloned()
                            .map(SessionDetail::ShiftableLoad)
                    }
                }
            });
            UserRequestWithSession {
                request: req,
                session,
            }
        })
        .collect();

    Json(enriched)
}

/// POST /user-requests — create a user energy task request (Stage 5).
///
/// Handles three asset types:
/// - Shiftable loads (WM etc.): detected by `power_kw + duration_min` fields; fast-path
///   that bypasses `create_from_body` (WM has no sim-asset profile entry).
/// - EV: `asset_id == "ev"` — goes through `create_from_body`.
/// - Heater: `asset_id == "heater" | "boiler"` — goes through `create_from_body`.
pub async fn post_requests(
    State(ctx): State<AppCtx>,
    Json(body): Json<CreateUserRequestBody>,
) -> impl IntoResponse {
    let now = Utc::now();

    // ── Shiftable-load fast-path (Plan C) ───────────────────────────────────
    // WM has no sim-asset profile entry; create_from_body would return UnknownAsset.
    if body.power_kw.is_some() && body.duration_min.is_some() {
        let earliest = body.earliest_start.unwrap_or(now);
        let latest = match body.latest_end {
            Some(t) => t,
            None => {
                return (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(serde_json::json!({"error": "latest_end required for shiftable load"})),
                )
                    .into_response()
            }
        };
        let power = body.power_kw.unwrap();
        let duration = body.duration_min.unwrap();
        let mode = body.mode.clone().unwrap_or_default();
        let load = ShiftableLoad {
            id: Uuid::new_v4(),
            asset_id: body.asset_id.clone(),
            power_kw: power,
            duration_min: duration,
            earliest_start: earliest,
            latest_end: latest,
            mode: mode.clone(),
            created_at: now,
            updated_at: now,
        };
        let user_req = UserRequest {
            id: Uuid::new_v4(),
            asset_id: body.asset_id.clone(),
            target_soc: None,
            target_energy_kwh: (power * duration as f64) / 60.0,
            desired_power_kw: power,
            deadlines: vec![],
            mode,
            completion_policy: "STOP".to_string(),
            max_total_cost_eur: None,
            tier_count: 0,
            session_id: Some(load.id),
            session_type: Some(SessionType::ShiftableLoad),
            status: UserRequestStatus::Active,
            estimated_cost_eur: 0.0,
            estimated_co2_g: 0.0,
            interruptible: body.interruptible.unwrap_or(false),
            tolerance_min: body.tolerance_min,
            budget_eur: body.budget_eur,
            created_at: now,
            updated_at: now,
        };
        if let Err(msg) = ctx.state.add_shiftable_load(load).await {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({"error": msg})),
            )
                .into_response();
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
        info!(
            request_id = %user_req.id,
            session_id = ?user_req.session_id,
            asset_id = %user_req.asset_id,
            power_kw = power,
            duration_min = duration,
            "user request created (shiftable load)"
        );
        return (
            StatusCode::CREATED,
            Json(serde_json::to_value(user_req).unwrap_or_default()),
        )
            .into_response();
    }

    // ── EV / heater path — requires sim-asset lookup ────────────────────────
    let asset_data: Vec<AssetRequestSlice> = {
        use crate::assets::{AssetConfig as AC, AssetState as AS};
        let sim = ctx.sim.lock().await;
        sim.assets
            .iter()
            .zip(sim.asset_configs.iter())
            .map(|(entry, cfg)| {
                let (current_soc, default_soc_target, capacity_kwh, max_charge_kw) =
                    match (&entry.state, cfg) {
                        (AS::Ev(s), AC::Ev(c)) => (
                            Some(s.soc),
                            Some(c.soc_target),
                            Some(c.battery_kwh),
                            Some(c.max_charge_kw),
                        ),
                        (AS::Battery(s), AC::Battery(c)) => (
                            Some(s.soc),
                            Some(1.0),
                            Some(c.capacity_kwh),
                            Some(c.max_charge_kw),
                        ),
                        _ => (None, None, None, None),
                    };
                AssetRequestSlice {
                    id: entry.id.clone(),
                    current_soc,
                    default_soc_target,
                    capacity_kwh,
                    max_charge_kw,
                    completion_policy: cfg.default_completion_policy(),
                    comfort_rates: cfg.default_comfort_rates(),
                }
            })
            .collect()
    };

    if UserRequestService::is_ev(&body) {
        match UserRequestService::create_ev(body, &asset_data, now) {
            Ok((user_req, session)) => {
                ctx.state.set_ev_session(Some(session.clone())).await;
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
                    StatusCode::CREATED,
                    Json(serde_json::to_value(user_req).unwrap_or_default()),
                )
                    .into_response()
            }
            Err(e) => {
                warn!("POST /user-requests (EV) rejected: {e}");
                (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(serde_json::json!({"error": e.to_string()})),
                )
                    .into_response()
            }
        }
    } else if UserRequestService::is_heater(&body) {
        match UserRequestService::create_heater(body, &asset_data, now) {
            Ok((user_req, target)) => {
                ctx.state.set_heater_target(Some(target.clone())).await;
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
                    StatusCode::CREATED,
                    Json(serde_json::to_value(user_req).unwrap_or_default()),
                )
                    .into_response()
            }
            Err(e) => {
                warn!("POST /user-requests (heater) rejected: {e}");
                (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(serde_json::json!({"error": e.to_string()})),
                )
                    .into_response()
            }
        }
    } else {
        warn!("POST /user-requests: unrecognised asset_id for EV/heater path");
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": "unrecognised asset type for EV/heater path"})),
        )
            .into_response()
    }
}

/// DELETE /user-requests/:id — cancel a user request and clear any linked device session.
pub async fn delete_request(State(ctx): State<AppCtx>, Path(id): Path<Uuid>) -> impl IntoResponse {
    match UserRequestService::cancel(id, &ctx.state).await {
        Ok(req) => {
            ctx.state
                .push_controller_event(
                    crate::controller::trace::ControllerEvent::RequestTransition {
                        ts: Utc::now(),
                        request_id: id,
                        asset_id: req.asset_id.clone(),
                        from_status: "Active".to_string(),
                        to_status: "Cancelled".to_string(),
                    },
                )
                .await;
            let _ = ctx.trigger_tx.send(PlanTrigger::UserRequest);
            info!(request_id = %id, "user request cancelled");
            axum::http::StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => {
            let (status, body) = e.into();
            (status, body).into_response()
        }
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
