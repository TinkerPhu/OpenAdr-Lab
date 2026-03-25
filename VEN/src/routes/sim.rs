use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;

use crate::entities::asset::PlanTrigger;
use crate::state::{SimInjectState, UserOverrides};
use crate::AppCtx;

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
    let schema: std::collections::HashMap<
        String,
        Vec<crate::simulator::assets::ControlDescriptor>,
    > = sim
        .iter_assets()
        .map(|(entry, cfg)| (entry.id.clone(), cfg.control_schema()))
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
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "soc must be between 0.0 and 1.0"})),
        )
            .into_response();
    }
    let mut sim = ctx.sim.lock().await;
    match sim.find_asset_mut(&asset_id) {
        Some((entry, cfg)) => {
            let mut values = std::collections::HashMap::new();
            values.insert("soc".to_string(), body.soc);
            cfg.reset(&mut entry.state, values);
            drop(sim);
            axum::http::StatusCode::NO_CONTENT.into_response()
        }
        None => (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("asset '{}' not found", asset_id)})),
        )
            .into_response(),
    }
}

/// PUT /sim/config/battery — update battery capacity_kwh and/or min_soc.
pub async fn put_sim_config_battery(
    State(ctx): State<AppCtx>,
    Json(body): Json<BatteryConfigBody>,
) -> impl IntoResponse {
    if body.capacity_kwh <= 0.0 {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "capacity_kwh must be > 0"})),
        )
            .into_response();
    }
    if let Some(min_soc) = body.min_soc {
        if !(0.0..=1.0).contains(&min_soc) {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "min_soc must be between 0.0 and 1.0"})),
            )
                .into_response();
        }
    }
    let mut sim = ctx.sim.lock().await;
    match sim.find_asset_mut("battery") {
        Some((_entry, cfg)) => {
            let mut values = std::collections::HashMap::new();
            values.insert("capacity_kwh".to_string(), body.capacity_kwh);
            if let Some(min_soc) = body.min_soc {
                values.insert("min_soc".to_string(), min_soc);
            }
            cfg.update_config(values);
            drop(sim);
            axum::http::StatusCode::NO_CONTENT.into_response()
        }
        None => (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "battery asset not found"})),
        )
            .into_response(),
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

/// GET /sim/override — backward-compat: translate inject_state back to UserOverrides shape.
/// `ev_plugged` is read by controller_v2_steps.py.
pub async fn get_sim_override(State(ctx): State<AppCtx>) -> impl IntoResponse {
    let inject = ctx.state.inject_state().await;
    let compat = UserOverrides {
        pv_irradiance: inject.pv_irradiance,
        ambient_temp_c: inject.ambient_temp_c,
        ev_plugged: inject.ev_plugged,
        base_load_w: inject.base_load_kw.map(|kw| kw * 1000.0),
        // Fields removed from the new inject model — always None
        ev_desired_kw: None,
        ev_max_charge_kw: None,
        ev_soc_target: None,
        heater_max_kw: None,
        heater_temp_min_c: None,
        heater_temp_max_c: None,
        pv_rated_kw: None,
    };
    Json(compat)
}

/// POST /sim/override — backward-compat alias; translates UserOverrides → SimInjectState.
/// Empty body `{}` releases all overrides (preserves existing reset behaviour).
pub async fn post_sim_override(
    State(ctx): State<AppCtx>,
    Json(body): Json<UserOverrides>,
) -> impl IntoResponse {
    let mut inject = SimInjectState::default();
    inject.ev_plugged = body.ev_plugged;
    inject.pv_irradiance = body.pv_irradiance;
    inject.ambient_temp_c = body.ambient_temp_c;
    if let Some(w) = body.base_load_w {
        inject.base_load_kw = Some(w / 1000.0);
    }
    // Silently drop removed fields: ev_max_charge_kw, ev_soc_target, ev_desired_kw,
    // heater_max_kw, heater_temp_min_c, heater_temp_max_c, pv_rated_kw.
    ctx.state.set_inject_state(inject).await;
    let _ = ctx.trigger_tx.send(PlanTrigger::AssetStateChange);
    axum::http::StatusCode::NO_CONTENT
}

/// GET /sim/inject — returns the current inject state.
pub async fn get_sim_inject(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.inject_state().await)
}

/// POST /sim/inject — partial-merge inject state.
/// Absent fields are unchanged; present fields replace current value (use null to release).
pub async fn post_sim_inject(
    State(ctx): State<AppCtx>,
    Json(body): Json<SimInjectState>,
) -> impl IntoResponse {
    ctx.state.set_inject_state(body).await;
    let _ = ctx.trigger_tx.send(PlanTrigger::AssetStateChange);
    axum::http::StatusCode::NO_CONTENT
}

/// POST /sim/inject/reset — release all active overrides at once.
pub async fn post_sim_inject_reset(State(ctx): State<AppCtx>) -> impl IntoResponse {
    ctx.state.set_inject_state(SimInjectState::default()).await;
    axum::http::StatusCode::NO_CONTENT
}
