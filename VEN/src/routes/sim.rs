use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use tracing::debug;

use crate::entities::asset::PlanTrigger;
use crate::state::SimInjectState;
use crate::AppCtx;

/// Partial-merge body for POST /sim/inject.
///
/// Serde semantics per field:
/// - Absent from JSON     → `None`              → no change to current state
/// - Present as `null`   → `Some(Value::Null)`  → release override
/// - Present as value    → `Some(Value::...)`   → activate override with that value
#[derive(Debug, Default, Deserialize)]
pub struct PostSimInjectBody {
    #[serde(default)]
    pub battery_soc: Option<serde_json::Value>,
    #[serde(default)]
    pub ev_soc: Option<serde_json::Value>,
    #[serde(default)]
    pub heater_temp_c: Option<serde_json::Value>,
    #[serde(default)]
    pub pv_irradiance: Option<serde_json::Value>,
    #[serde(default)]
    pub pv_irradiance_alpha: Option<serde_json::Value>,
    #[serde(default)]
    pub ev_plugged: Option<serde_json::Value>,
    #[serde(default)]
    pub ev_soc_target: Option<serde_json::Value>,
    #[serde(default)]
    pub heater_setpoint_c: Option<serde_json::Value>,
    #[serde(default)]
    pub heater_temp_min_c: Option<serde_json::Value>,
    #[serde(default)]
    pub heater_temp_max_c: Option<serde_json::Value>,
    #[serde(default)]
    pub ambient_temp_c: Option<serde_json::Value>,
    #[serde(default)]
    pub base_load_kw: Option<serde_json::Value>,
    #[serde(default)]
    pub base_load_alpha: Option<serde_json::Value>,
    #[serde(default)]
    pub grid_import_limit_kw: Option<serde_json::Value>,
    #[serde(default)]
    pub grid_export_limit_kw: Option<serde_json::Value>,
    #[serde(default)]
    pub pv_plan_kw: Option<serde_json::Value>,
}

/// Apply partial-merge: absent = no change, null = release (None), value = set.
fn merge_inject(current: &mut SimInjectState, body: PostSimInjectBody) {
    macro_rules! merge_f64 {
        ($field:ident) => {
            if let Some(v) = body.$field {
                current.$field = if v.is_null() { None } else { v.as_f64() };
            }
        };
    }
    macro_rules! merge_bool {
        ($field:ident) => {
            if let Some(v) = body.$field {
                current.$field = if v.is_null() { None } else { v.as_bool() };
            }
        };
    }
    merge_f64!(battery_soc);
    merge_f64!(ev_soc);
    merge_f64!(heater_temp_c);
    merge_f64!(pv_irradiance);
    if let Some(v) = body.pv_irradiance_alpha {
        if let Some(alpha) = v.as_f64() {
            current.pv_irradiance_alpha = alpha;
        } else if v.is_null() {
            current.pv_irradiance_alpha = 0.1; // reset to default
        }
    }
    merge_bool!(ev_plugged);
    merge_f64!(ev_soc_target);
    merge_f64!(heater_setpoint_c);
    merge_f64!(heater_temp_min_c);
    merge_f64!(heater_temp_max_c);
    merge_f64!(ambient_temp_c);
    merge_f64!(base_load_kw);
    if let Some(v) = body.base_load_alpha {
        if let Some(alpha) = v.as_f64() {
            current.base_load_alpha = alpha;
        } else if v.is_null() {
            current.base_load_alpha = 0.1; // reset to default
        }
    }
    merge_f64!(grid_import_limit_kw);
    merge_f64!(grid_export_limit_kw);
    merge_f64!(pv_plan_kw);
}

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
///
/// Reads from profile config only — does NOT acquire the sim mutex, so it
/// remains responsive even while the MILP planner is running (10-24s on Pi4).
pub async fn get_sim_schema(State(ctx): State<AppCtx>) -> impl IntoResponse {
    debug!("GET /sim/schema: building schema from profile (no sim lock)");
    let schema = crate::simulator::schema_from_profile(&ctx.profile);
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
    match sim.find_asset_mut(crate::ids::ASSET_BATTERY) {
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

/// GET /sim/inject — returns the current inject state.
pub async fn get_sim_inject(State(ctx): State<AppCtx>) -> impl IntoResponse {
    Json(ctx.state.inject_state().await)
}

/// POST /sim/inject — partial-merge inject state.
/// Absent fields are unchanged; `null` releases the override; a value activates it.
pub async fn post_sim_inject(
    State(ctx): State<AppCtx>,
    Json(body): Json<PostSimInjectBody>,
) -> impl IntoResponse {
    // Trigger a replan only for fields the MILP planner uses as inputs.
    // base_load_kw / base_load_alpha are one-shot physics overrides for test deviation
    // simulation — triggering a replan on them would corrupt the absorber's assertion
    // window by adopting a new plan mid-test.
    // pv_plan_kw is a planning-only forecast pin — it takes effect on the *next* scheduled
    // solve cycle and must NOT trigger an immediate replan, which would race the BDD
    // assertion window exactly like base_load_kw does.
    let should_replan = body.pv_irradiance.is_some()
        || body.battery_soc.is_some()
        || body.ev_soc.is_some()
        || body.ev_plugged.is_some()
        || body.ev_soc_target.is_some()
        || body.heater_temp_c.is_some()
        || body.heater_setpoint_c.is_some()
        || body.ambient_temp_c.is_some()
        || body.grid_import_limit_kw.is_some()
        || body.grid_export_limit_kw.is_some();
    let mut current = ctx.state.inject_state().await;
    merge_inject(&mut current, body);
    ctx.state.set_inject_state(current).await;
    if should_replan {
        let _ = ctx.trigger_tx.send(PlanTrigger::AssetStateChange);
    }
    axum::http::StatusCode::NO_CONTENT
}

/// POST /plan/trigger — force an immediate MILP replan.
///
/// Sends `PlanTrigger::AssetStateChange` without modifying any sim state.
/// Useful in tests to request a fresh plan without side-effecting physics
/// (e.g., after calling `POST /sim/reset` or adjusting an EV session).
pub async fn post_plan_trigger(State(ctx): State<AppCtx>) -> impl IntoResponse {
    let _ = ctx.trigger_tx.send(PlanTrigger::AssetStateChange);
    axum::http::StatusCode::NO_CONTENT
}

/// POST /sim/inject/reset — release all active overrides at once.
pub async fn post_sim_inject_reset(State(ctx): State<AppCtx>) -> impl IntoResponse {
    ctx.state.set_inject_state(SimInjectState::default()).await;
    axum::http::StatusCode::NO_CONTENT
}
