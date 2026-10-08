use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use tracing::warn;
use uuid::Uuid;

use super::{SessionDetail, UserRequestWithSession};
use crate::app_ctx::{PlanTriggerTx, Roster, SimRead};
use crate::controller::user_request::{
    ComfortRateParams, CreateUserRequestParams, RequestDeadlineParams,
};
use crate::entities::design_vocabulary::UserRequestMode;
use crate::entities::user_request::SessionType;
use crate::services::request_submission::{self, SubmitError};
use crate::state::AppState;

/// R-25: HTTP DTO for POST /user-requests — owned by the routes layer.
/// Converts into the domain-owned `CreateUserRequestParams` before crossing
/// into `controller::user_request`/`services::user_request`.
#[derive(Debug, Deserialize)]
pub struct CreateUserRequestBody {
    pub asset_id: String,
    #[serde(rename = "target_soc")]
    pub target_soc_frac: Option<f64>,
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
            target_soc_frac: b.target_soc_frac,
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
pub async fn get_requests(State(state): State<AppState>) -> impl IntoResponse {
    let requests = state.active_requests().await;
    // The whole queue: each request resolves to the session it owns, by id. Matching
    // against one global session meant that with several queued, only whichever
    // happened to be stored could ever be shown - the rest reported no session at all.
    let ev = state.ev_sessions().await;
    let heater = state.heater_target().await;
    let loads = state.shiftable_loads().await;

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

/// POST /user-requests — create a user energy task request (shiftable load, EV or heater;
/// `services::request_submission::submit` decides which).
pub async fn post_requests(
    State(state): State<AppState>,
    State(roster): State<Roster>,
    State(sim_read): State<SimRead>,
    State(trigger_tx): State<PlanTriggerTx>,
    Json(body): Json<CreateUserRequestBody>,
) -> impl IntoResponse {
    let submitted = request_submission::submit(
        body.into(),
        Utc::now(),
        &state,
        roster.as_ref(),
        sim_read.as_ref(),
        &trigger_tx,
    )
    .await;
    match submitted {
        Ok(user_req) => (StatusCode::CREATED, Json(user_req)).into_response(),
        Err(e) => {
            warn!("POST /user-requests refused: {e}");
            submit_error_response(e)
        }
    }
}

/// An EV clash keeps its 409 wire contract (`ev_conflict::conflict_response`); a duplicate
/// shiftable load is 409; everything else the request got wrong is 422.
fn submit_error_response(e: SubmitError) -> axum::response::Response {
    let error = |status: StatusCode, message: String| {
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    };
    match e {
        SubmitError::Request(e) => super::ev_conflict::conflict_response(&e)
            .unwrap_or_else(|| error(StatusCode::UNPROCESSABLE_ENTITY, e.to_string())),
        SubmitError::Duplicate(msg) => error(StatusCode::CONFLICT, msg),
        SubmitError::Invalid(_) | SubmitError::UnrecognisedAsset => {
            error(StatusCode::UNPROCESSABLE_ENTITY, e.to_string())
        }
    }
}

/// DELETE /user-requests/:id — cancel a user request and clear any linked device session.
pub async fn delete_request(
    State(state): State<AppState>,
    State(roster): State<Roster>,
    State(trigger_tx): State<PlanTriggerTx>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let cancelled = request_submission::cancel_and_announce(
        id,
        Utc::now(),
        &state,
        roster.as_ref(),
        &trigger_tx,
    )
    .await;
    match cancelled {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            let (status, body) = e.into();
            (status, body).into_response()
        }
    }
}
