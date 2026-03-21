use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;

use crate::AppCtx;
use crate::state::UserOverrides;

#[derive(Deserialize)]
pub struct SocBody {
    pub soc: f64,
}

#[derive(Deserialize)]
pub struct BatteryConfigBody {
    pub capacity_kwh: f64,
    pub min_soc: Option<f64>,
}

/// GET /sim/schema — returns control descriptors for all configured assets.
pub async fn get_sim_schema(State(ctx): State<AppCtx>) -> impl IntoResponse {
    let sim = ctx.sim.lock().await;
    let schema: std::collections::HashMap<String, Vec<crate::simulator::assets::ControlDescriptor>> = sim
        .assets
        .iter()
        .map(|entry| (entry.id.clone(), entry.state.control_schema()))
        .collect();
    Json(schema)
}

/// POST /sim/reset/:asset_id — jump an asset's SoC to the given value.
pub async fn post_sim_reset(
    State(ctx): State<AppCtx>,
    Path(asset_id): Path<String>,
    Json(body): Json<SocBody>,
) -> impl IntoResponse {
    if !(0.0..=1.0).contains(&body.soc) {
        return (axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "soc must be between 0.0 and 1.0"}))).into_response();
    }
    let mut sim = ctx.sim.lock().await;
    match sim.asset_mut(&asset_id) {
        Some(entry) => {
            let mut values = std::collections::HashMap::new();
            values.insert("soc".to_string(), body.soc);
            entry.state.reset(values);
            drop(sim);
            axum::http::StatusCode::NO_CONTENT.into_response()
        }
        None => (axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("asset '{}' not found", asset_id)}))).into_response(),
    }
}

/// PUT /sim/config/battery — update battery capacity_kwh and/or min_soc.
pub async fn put_sim_config_battery(
    State(ctx): State<AppCtx>,
    Json(body): Json<BatteryConfigBody>,
) -> impl IntoResponse {
    if body.capacity_kwh <= 0.0 {
        return (axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "capacity_kwh must be > 0"}))).into_response();
    }
    if let Some(min_soc) = body.min_soc {
        if !(0.0..=1.0).contains(&min_soc) {
            return (axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "min_soc must be between 0.0 and 1.0"}))).into_response();
        }
    }
    let mut sim = ctx.sim.lock().await;
    match sim.asset_mut("battery") {
        Some(entry) => {
            let mut values = std::collections::HashMap::new();
            values.insert("capacity_kwh".to_string(), body.capacity_kwh);
            if let Some(min_soc) = body.min_soc {
                values.insert("min_soc".to_string(), min_soc);
            }
            entry.state.update_config(values);
            drop(sim);
            axum::http::StatusCode::NO_CONTENT.into_response()
        }
        None => (axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "battery asset not found"}))).into_response(),
    }
}

pub async fn get_sim(State(ctx): State<AppCtx>) -> impl IntoResponse {
    match ctx.state.sim().await {
        Some(sim) => Json(serde_json::to_value(sim).unwrap_or_default()).into_response(),
        None => (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "simulator not yet initialized"})),
        )
            .into_response(),
    }
}

pub async fn get_sim_override(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.overrides().await)
}

pub async fn post_sim_override(
    State(ctx): State<AppCtx>,
    Json(body): Json<UserOverrides>,
) -> impl IntoResponse {
    ctx.state.set_overrides(body).await;
    axum::http::StatusCode::NO_CONTENT
}
