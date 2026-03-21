use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use serde::Deserialize;
use tracing::{info, warn};
use uuid::Uuid;

use crate::AppCtx;
use crate::controller::user_request::CreateUserRequestBody;
use crate::entities::asset::{ComfortRate, PlanTrigger};
use crate::entities::energy_packet::{DeadlineTier, EnergyPacket, ValueCurve};

/// POST /packets body shape (Stage 4).
#[derive(Deserialize)]
pub struct CreatePacketRequest {
    pub asset_id: String,
    pub target_energy_kwh: Option<f64>,
    pub target_soc: Option<f64>,
    pub desired_power_kw: Option<f64>,
    pub latest_end: Option<chrono::DateTime<Utc>>,
}

/// GET /packets — returns active EnergyPackets (empty until Stage 3).
pub async fn get_packets(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.active_packets().await)
}

/// GET /plan — returns the active Plan (null until Stage 3).
pub async fn get_plan(State(ctx): State<AppCtx>) -> impl IntoResponse {
    match ctx.state.active_plan().await {
        Some(plan) => Json(serde_json::to_value(plan).unwrap_or_default()).into_response(),
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

/// POST /packets — create a new EnergyPacket and trigger a replan (Stage 4).
pub async fn post_packets(
    State(ctx): State<AppCtx>,
    Json(body): Json<CreatePacketRequest>,
) -> impl IntoResponse {
    let now = Utc::now();
    let desired_power_kw = body.desired_power_kw.unwrap_or(1.0);
    let target_energy_kwh = body.target_energy_kwh.unwrap_or(desired_power_kw); // default: 1h

    let value_curve = ValueCurve {
        comfort_rates: vec![
            ComfortRate { fill: 0.0, max_marginal_price: 0.35, max_marginal_co2: 0.0 },
            ComfortRate { fill: 1.0, max_marginal_price: 0.05, max_marginal_co2: 0.0 },
        ],
        deadline_tiers: body
            .latest_end
            .map(|le| {
                vec![DeadlineTier {
                    deadline: le,
                    max_total_cost_eur: None,
                    max_marginal_rate_eur_kwh: None,
                    min_completion: 0.8,
                }]
            })
            .unwrap_or_default(),
        active_tier_index: 0,
    };
    let packet = EnergyPacket {
        target_soc: body.target_soc,
        ..EnergyPacket::new(body.asset_id, target_energy_kwh, desired_power_kw, value_curve, now)
    };

    let mut packets = ctx.state.active_packets().await;
    packets.push(packet.clone());
    ctx.state.set_active_packets(packets).await;

    // Signal the planning loop: a new packet needs scheduling
    let _ = ctx.trigger_tx.send(PlanTrigger::UserRequest);

    info!(asset_id = %packet.asset_id, packet_id = %packet.id, "new EnergyPacket created via POST /packets");
    (axum::http::StatusCode::CREATED, Json(packet))
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
    let assets = ctx.sim.lock().await.assets.clone();

    match crate::controller::user_request::create_from_body(body, &assets, now) {
        Ok((user_req, packet)) => {
            info!(
                request_id = %user_req.id,
                packet_id = %packet.id,
                asset_id = %packet.asset_id,
                target_kwh = packet.target_energy_kwh,
                "user request created"
            );
            let mut packets = ctx.state.active_packets().await;
            packets.push(packet);
            ctx.state.set_active_packets(packets).await;
            ctx.state.upsert_request(user_req.clone()).await;
            // T044: emit RequestTransition for new request
            ctx.state.push_controller_event(
                crate::controller::trace::ControllerEvent::RequestTransition {
                    ts: now,
                    request_id: user_req.id,
                    asset_id: user_req.asset_id.clone(),
                    from_status: "None".to_string(),
                    to_status: format!("{:?}", user_req.status),
                },
            ).await;
            let _ = ctx.trigger_tx.send(PlanTrigger::UserRequest);
            (axum::http::StatusCode::CREATED, Json(serde_json::to_value(user_req).unwrap_or_default())).into_response()
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

/// DELETE /user-requests/:id — cancel a user request and abandon its packet (Stage 5).
pub async fn delete_request(
    State(ctx): State<AppCtx>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match ctx.state.cancel_request(id).await {
        Some(packet_id) => {
            // abandon_packet is now atomic inside cancel_request
            // T044: emit RequestTransition for cancellation
            ctx.state.push_controller_event(
                crate::controller::trace::ControllerEvent::RequestTransition {
                    ts: Utc::now(),
                    request_id: id,
                    asset_id: String::new(),
                    from_status: "Active".to_string(),
                    to_status: "Cancelled".to_string(),
                },
            ).await;
            let _ = ctx.trigger_tx.send(PlanTrigger::UserRequest);
            info!(request_id = %id, packet_id = %packet_id, "user request cancelled");
            axum::http::StatusCode::NO_CONTENT.into_response()
        }
        None => (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "request not found"})),
        )
            .into_response(),
    }
}

/// GET /flexibility — returns FlexibilityEnvelopes from the active plan (Stage 5).
pub async fn get_flexibility(State(ctx): State<AppCtx>) -> impl IntoResponse {
    match ctx.state.active_plan().await {
        Some(plan) => Json(serde_json::to_value(plan.envelopes).unwrap_or_default()).into_response(),
        None => Json(serde_json::json!([])).into_response(),
    }
}
