use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use tracing::{info, warn};
use uuid::Uuid;

use super::{SessionDetail, UserRequestWithSession};
use crate::controller::user_request::{
    ClashingSession, ComfortRateParams, CreateUserRequestParams, RequestDeadlineParams,
    RequestError,
};
use crate::entities::asset::{PlanTrigger, PlanTriggerSignal};
use crate::entities::asset_params::AssetRequestSlice;
use crate::entities::design_vocabulary::UserRequestMode;
use crate::entities::user_request::SessionType;
use crate::services::user_request::UserRequestService;
use crate::AppCtx;

/// R-25: HTTP DTO for POST /user-requests — owned by the routes layer.
/// Converts into the domain-owned `CreateUserRequestParams` before crossing
/// into `controller::user_request`/`services::user_request`.
#[derive(Debug, Deserialize)]
pub struct CreateUserRequestBody {
    pub asset_id: String,
    pub target_soc: Option<f64>,
    pub target_energy_kwh: Option<f64>,
    pub desired_power_kw: Option<f64>,
    pub deadlines: Vec<RequestDeadlineInput>,
    pub completion_policy: Option<String>,
    pub comfort_rates: Option<Vec<ComfortRateInput>>,
    // ── Leeway fields (§8.2) ────────────────────────────────────────────────
    pub budget_eur: Option<f64>,     // top-level cost ceiling shorthand
    pub interruptible: Option<bool>, // planner may pause/resume
    pub tolerance_min: Option<i64>,  // ±N minutes around deadline acceptable
    // ── Shiftable-load fields (Plan C) ──────────────────────────────────────
    pub power_kw: Option<f64>,
    pub duration_min: Option<u32>,
    pub earliest_start: Option<DateTime<Utc>>,
    /// EV only: km expected after this session's departure. Absent = use the EV's
    /// own default, and say so in the plan rather than substituting silently.
    pub expected_trip_distance_km: Option<f64>,
    /// EV only: when the car is expected back from that trip. Paired with the
    /// distance above; a half-stated estimate is refused rather than completed by a
    /// guess.
    pub expected_return_time: Option<DateTime<Utc>>,
    pub latest_end: Option<DateTime<Utc>>,
    // ── Per-device overrides (Plan D) ────────────────────────────────────────
    pub soft_deadline: Option<bool>,
    pub target_temp_c: Option<f64>,
    // ── Request mode (BL-28) — omitted = BY_DEADLINE (legacy behaviour) ─────
    pub mode: Option<UserRequestMode>,
    /// EV only: the queued sessions this submission intends to displace.
    ///
    /// Absent means "displace nothing" — a clash is then refused, which is the
    /// default and the safe answer. Present, it must name exactly the sessions the
    /// candidate clashes with, so the confirmation refers to the plans the user was
    /// actually shown rather than to whatever happens to clash when the server gets
    /// around to it (`ev-session-conflict-resolution`).
    #[serde(default)]
    pub replace_session_ids: Option<Vec<uuid::Uuid>>,
}

#[derive(Debug, Deserialize)]
pub struct RequestDeadlineInput {
    pub latest_end: DateTime<Utc>,
    pub max_total_cost_eur: Option<f64>,
    pub max_marginal_rate_eur_kwh: Option<f64>,
    pub min_completion: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct ComfortRateInput {
    pub fill: f64,
    pub bid: f64,
    /// Max gCO2/kWh the user accepts at this fill level (BL-17 comfort bidding).
    /// `None` = no CO2 preference expressed, treated as 0.0.
    pub co2: Option<f64>,
}

impl From<RequestDeadlineInput> for RequestDeadlineParams {
    fn from(d: RequestDeadlineInput) -> Self {
        RequestDeadlineParams {
            latest_end: d.latest_end,
            max_total_cost_eur: d.max_total_cost_eur,
            max_marginal_rate_eur_kwh: d.max_marginal_rate_eur_kwh,
            min_completion: d.min_completion,
        }
    }
}

impl From<ComfortRateInput> for ComfortRateParams {
    fn from(c: ComfortRateInput) -> Self {
        ComfortRateParams {
            fill: c.fill,
            bid: c.bid,
            co2: c.co2,
        }
    }
}

impl From<CreateUserRequestBody> for CreateUserRequestParams {
    fn from(b: CreateUserRequestBody) -> Self {
        CreateUserRequestParams {
            asset_id: b.asset_id,
            target_soc: b.target_soc,
            target_energy_kwh: b.target_energy_kwh,
            desired_power_kw: b.desired_power_kw,
            deadlines: b.deadlines.into_iter().map(Into::into).collect(),
            completion_policy: b.completion_policy,
            comfort_rates: b
                .comfort_rates
                .map(|rates| rates.into_iter().map(Into::into).collect()),
            budget_eur: b.budget_eur,
            interruptible: b.interruptible,
            tolerance_min: b.tolerance_min,
            power_kw: b.power_kw,
            duration_min: b.duration_min,
            earliest_start: b.earliest_start,
            expected_trip_distance_km: b.expected_trip_distance_km,
            expected_return_time: b.expected_return_time,
            latest_end: b.latest_end,
            soft_deadline: b.soft_deadline,
            target_temp_c: b.target_temp_c,
            mode: b.mode,
            replace_session_ids: b.replace_session_ids,
        }
    }
}

/// GET /user-requests — list all user requests with embedded session details.
pub async fn get_requests(State(ctx): State<AppCtx>) -> impl IntoResponse {
    let requests = ctx.state.active_requests().await;
    // The whole queue: each request resolves to the session it owns, by id. Matching
    // against one global session meant that with several queued, only whichever
    // happened to be stored could ever be shown - the rest reported no session at all.
    let ev = ctx.state.ev_sessions().await;
    let heater = ctx.state.heater_target().await;
    let loads = ctx.state.shiftable_loads().await;

    let enriched: Vec<UserRequestWithSession> = requests
        .into_iter()
        .map(|req| {
            let session = req.session_id.and_then(|sid| {
                match req.session_type {
                    Some(SessionType::Ev) => ev
                        .iter()
                        .find(|s| s.id == sid)
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
                        if let Some(s) = ev.iter().find(|s| s.id == sid) {
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
    let body: CreateUserRequestParams = body.into();
    let now = Utc::now();

    // ── Shiftable-load fast-path (Plan C) ───────────────────────────────────
    // WM has no sim-asset profile entry; create_from_body would return UnknownAsset.
    if UserRequestService::is_shiftable(&body) {
        // The request/load pair and every rule about it belong to the
        // service; this route's job is to turn its `Err` into a status code
        // and to install what it returns.
        let (user_req, load) = match UserRequestService::create_shiftable(body, now) {
            Ok(pair) => pair,
            Err(msg) => {
                return (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(serde_json::json!({ "error": msg })),
                )
                    .into_response()
            }
        };
        if let Err(msg) = ctx.state.add_shiftable_load(load.clone()).await {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({"error": msg})),
            )
                .into_response();
        }
        // shiftable-load-as-asset design.md D1: the asset enters SimState at
        // acceptance time (started = false), not deferred until the MILP
        // picks a start slot — visible to forecasting/MILP for its whole life.
        if let Err(msg) = ctx.roster.add_shiftable(&load).await {
            // Should be unreachable: add_shiftable_load's own duplicate
            // check above already rejects a reused asset_id.
            warn!(asset_id = %load.asset_id, error = %msg, "add_asset failed after add_shiftable_load succeeded");
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
        let _ = ctx
            .trigger_tx
            .send(PlanTriggerSignal::bare(PlanTrigger::UserRequest));
        info!(
            request_id = %user_req.id,
            session_id = ?user_req.session_id,
            asset_id = %user_req.asset_id,
            power_kw = load.power_kw,
            duration_min = load.duration_min,
            "user request created (shiftable load)"
        );
        return (
            StatusCode::CREATED,
            Json(serde_json::to_value(user_req).unwrap_or_default()),
        )
            .into_response();
    }

    // ── EV / heater path — requires sim-asset lookup ────────────────────────
    // WP4.2 (BL-19): user comfort-curve overrides beat the built-in defaults.
    let comfort_overrides = ctx.state.comfort_overrides_map().await;
    let mut asset_data: Vec<AssetRequestSlice> = ctx.sim_read.request_slices().await;
    for slice in &mut asset_data {
        slice.comfort_rates = crate::services::comfort::effective_comfort_rates(
            &comfort_overrides,
            &slice.id,
            std::mem::take(&mut slice.comfort_rates),
        );
    }

    if UserRequestService::is_ev(&body) {
        // Read before `body` is moved: a stated instruction is what separates
        // "refuse the clash" from "displace exactly these".
        let replace_ids = body.replace_session_ids.clone();
        match UserRequestService::create_ev(body, &asset_data, now) {
            Ok((user_req, session)) => {
                // A clash is never displaced silently. Without an instruction the
                // submission is refused with the clashing plans named, so the user is
                // offered the replacement instead of a search; with one, exactly the
                // named sessions go, atomically, and only if they are still exactly
                // what this session clashes with.
                let outcome = match &replace_ids {
                    Some(ids) => ctx
                        .state
                        .replace_ev_sessions(ids, session.clone())
                        .await
                        .map(|_| ())
                        .map_err(|refusal| RequestError::EvReplaceRejected {
                            rejection: refusal.rejection,
                            conflicts: refusal.conflicts.iter().map(ClashingSession::of).collect(),
                        }),
                    None => ctx
                        .state
                        .insert_ev_session(session.clone())
                        .await
                        .map_err(|clash| RequestError::EvSessionsConflict {
                            conflicts: clash.conflicts.iter().map(ClashingSession::of).collect(),
                        }),
                };
                if let Err(e) = outcome {
                    warn!("POST /user-requests (EV) refused: {e}");
                    if let Some(resp) = super::ev_conflict::conflict_response(&e) {
                        return resp;
                    }
                    return (
                        StatusCode::UNPROCESSABLE_ENTITY,
                        Json(serde_json::json!({ "error": e.to_string() })),
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
                let _ = ctx
                    .trigger_tx
                    .send(PlanTriggerSignal::bare(PlanTrigger::UserRequest));
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
                let _ = ctx
                    .trigger_tx
                    .send(PlanTriggerSignal::bare(PlanTrigger::UserRequest));
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
    match UserRequestService::cancel(id, &ctx.state, ctx.roster.as_ref()).await {
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
            let _ = ctx
                .trigger_tx
                .send(PlanTriggerSignal::bare(PlanTrigger::UserRequest));
            info!(request_id = %id, "user request cancelled");
            axum::http::StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => {
            let (status, body) = e.into();
            (status, body).into_response()
        }
    }
}
