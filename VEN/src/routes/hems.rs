use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use tracing::{info, warn};
use uuid::Uuid;

use crate::controller::user_request::CreateUserRequestBody;
use crate::entities::asset::PlanTrigger;
use crate::entities::asset_params::AssetRequestSlice;
use crate::entities::device_session::{
    BaselineOverride, BaselineSlot, EvSession, HeaterTarget, ShiftableLoad,
};
use crate::entities::user_request::{SessionType, UserRequest, UserRequestStatus};
use crate::entities::PlannerObjective;
use crate::services::hems::EvSessionService;
use crate::services::user_request::UserRequestService;
use crate::AppCtx;

/// Embedded session detail for GET /user-requests response.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionDetail {
    Ev(EvSession),
    Heater(HeaterTarget),
    ShiftableLoad(ShiftableLoad),
}

/// Enriched user request with embedded session details.
#[derive(Debug, Clone, Serialize)]
pub struct UserRequestWithSession {
    #[serde(flatten)]
    pub request: UserRequest,
    pub session: Option<SessionDetail>,
}

/// GET /plan — returns the active Plan (null until Stage 3).
pub async fn get_plan(State(ctx): State<AppCtx>) -> impl IntoResponse {
    match ctx.state.active_plan().await {
        Some(plan) => Json(serde_json::to_value(plan).unwrap_or_default()).into_response(),
        None => Json(serde_json::Value::Null).into_response(),
    }
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

/// GET /obligations — returns pending report obligations (Stage 2).
pub async fn get_obligations(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.report_obligations().await)
}

/// GET /ledger — returns per-asset cumulative energy/cost/CO₂ (Stage 4).
pub async fn get_ledger(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.asset_ledger().await)
}

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
        let load = ShiftableLoad {
            id: Uuid::new_v4(),
            asset_id: body.asset_id.clone(),
            power_kw: power,
            duration_min: duration,
            earliest_start: earliest,
            latest_end: latest,
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
                        (AS::Ev(s), AC::Ev(c)) => {
                            (Some(s.soc), Some(c.soc_target), Some(c.battery_kwh), Some(c.max_charge_kw))
                        }
                        (AS::Battery(s), AC::Battery(c)) => {
                            (Some(s.soc), Some(1.0), Some(c.capacity_kwh), Some(c.max_charge_kw))
                        }
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

// ── Device-centric session endpoints (Phase A) ──────────────────────────────

/// POST /ev-session body.
#[derive(Deserialize)]
pub struct CreateEvSessionBody {
    pub target_soc: f64,
    pub departure_time: chrono::DateTime<Utc>,
    /// If true, MILP treats charging as a soft reward (best-effort by departure).
    /// If false (default), charging is a hard constraint (must reach target SoC by departure).
    #[serde(default)]
    pub soft_deadline: bool,
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

// ── SSE: Planner status stream (Plan E) ─────────────────────────────────────

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
