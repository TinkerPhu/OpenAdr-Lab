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
    ComfortRateParams, CreateUserRequestParams, RequestDeadlineParams,
};
use crate::entities::asset::PlanTrigger;
use crate::entities::asset_params::AssetRequestSlice;
use crate::entities::design_vocabulary::UserRequestMode;
use crate::entities::device_session::ShiftableLoad;
use crate::entities::user_request::{SessionType, UserRequest, UserRequestStatus};
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
    pub latest_end: Option<DateTime<Utc>>,
    // ── Per-device overrides (Plan D) ────────────────────────────────────────
    pub soft_deadline: Option<bool>,
    pub target_temp_c: Option<f64>,
    // ── Request mode (BL-28) — omitted = BY_DEADLINE (legacy behaviour) ─────
    pub mode: Option<UserRequestMode>,
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
            latest_end: b.latest_end,
            soft_deadline: b.soft_deadline,
            target_temp_c: b.target_temp_c,
            mode: b.mode,
        }
    }
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
    let body: CreateUserRequestParams = body.into();
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
            comfort_rates: vec![],
            status: UserRequestStatus::Active,
            estimated_cost_eur: 0.0,
            estimated_co2_g: 0.0,
            accumulated_cost_eur: 0.0,
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
    // WP4.2 (BL-19): user comfort-curve overrides beat the built-in defaults.
    let comfort_overrides = ctx.state.comfort_overrides_map().await;
    let asset_data: Vec<AssetRequestSlice> = {
        use crate::assets::{AssetState as AS, Battery, EvCharger};
        let sim = ctx.sim.lock().await;
        sim.assets
            .iter()
            .zip(sim.asset_configs.iter())
            .map(|(entry, cfg)| {
                let (current_soc, default_soc_target, capacity_kwh, max_charge_kw) =
                    if let (AS::Ev(s), Some(c)) =
                        (&entry.state, cfg.as_any().downcast_ref::<EvCharger>())
                    {
                        (
                            Some(s.soc),
                            Some(c.soc_target),
                            Some(c.battery_kwh),
                            Some(c.max_charge_kw),
                        )
                    } else if let (AS::Battery(s), Some(c)) =
                        (&entry.state, cfg.as_any().downcast_ref::<Battery>())
                    {
                        (
                            Some(s.soc),
                            Some(1.0),
                            Some(c.capacity_kwh),
                            Some(c.max_charge_kw),
                        )
                    } else {
                        (None, None, None, None)
                    };
                AssetRequestSlice {
                    id: entry.id.clone(),
                    current_soc,
                    default_soc_target,
                    capacity_kwh,
                    max_charge_kw,
                    completion_policy: cfg.default_completion_policy(),
                    comfort_rates: crate::services::comfort::effective_comfort_rates(
                        &comfort_overrides,
                        &entry.id,
                        cfg.default_comfort_rates(),
                    ),
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

/// GET /flexibility/history — BL-43: the site-headroom ring
/// (`AppState::flexibility_history`), oldest first, for the "Site Headroom"
/// diagram. Distinct from `GET /flexibility`, which is a single live snapshot.
/// Always 200 — an empty array before the first dispatcher tick.
pub async fn get_flexibility_history(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.flexibility_history().await)
}

/// GET /flexibility/forecast — forward-looking per-slot headroom trajectory,
/// re-derived fresh every dispatcher tick from the active plan's own setpoint
/// schedule plus each asset's real current state (see
/// `SiteFlexibilityForecastSlot`'s doc comment) — distinct from both
/// `GET /flexibility` (instant-only) and `GET /flexibility/history` (the past
/// ring). Always 200 — an empty array when there's no active plan.
pub async fn get_flexibility_forecast(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.site_headroom_forecast().await)
}

/// GET /flexibility/capacity — sustained-commitment power/duration/energy
/// capacity curves (both directions in one response — see
/// `openspec/changes/flexibility-capacity-forecast/design.md` open question
/// 2), re-derived fresh every dispatcher tick from the current asset state
/// (see `controller::capacity_forecast`'s module doc for why this is a
/// distinct computation from `GET /flexibility/forecast` above, not an
/// extension of it). 204 before the first dispatcher tick.
pub async fn get_capacity_curves(State(ctx): State<AppCtx>) -> impl IntoResponse {
    match ctx.state.capacity_curves().await {
        Some((import_curve, export_curve)) => Json(serde_json::json!({
            "import": import_curve,
            "export": export_curve,
        }))
        .into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}
