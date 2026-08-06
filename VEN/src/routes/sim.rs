use axum::{
    extract::{ConnectInfo, Path, State},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Deserializer};
use std::net::SocketAddr;
use tracing::{debug, warn};

use crate::entities::asset::PlanTrigger;
use crate::entities::sim_inject::SimInjectState;
use crate::AppCtx;

/// Deserializes a field as `Option<Option<T>>` ("double option") so a
/// tri-state PATCH body can distinguish all three JSON shapes. Serde's
/// blanket `Option<T>` impl collapses a top-level JSON `null` straight to
/// Rust `None` for *any* `T` before `T::deserialize` runs, which makes a
/// plain `Option<T>` field unable to tell "absent" and "present as null"
/// apart. Wrapping the deserializer call in an extra `Option::deserialize`
/// here restores that distinction one level up.
fn double_option<'de, T, D>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Ok(Some(Option::deserialize(de)?))
}

/// Partial-merge body for POST /sim/inject.
///
/// Semantics per field:
/// - Absent from JSON  → `None`       → no change to current state
/// - Present as `null` → `Some(None)` → release override
/// - Present as value  → `Some(Some(v))` → activate override with that value
#[derive(Debug, Default, Deserialize)]
pub struct PostSimInjectBody {
    #[serde(default, deserialize_with = "double_option")]
    pub battery_soc: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub ev_soc: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub heater_temp_c: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub pv_irradiance: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub pv_irradiance_alpha: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub ev_plugged: Option<Option<bool>>,
    #[serde(default, deserialize_with = "double_option")]
    pub ev_soc_target: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub heater_setpoint_c: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub heater_temp_min_c: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub heater_temp_max_c: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub heater_emergency_curtail: Option<Option<bool>>,
    #[serde(default, deserialize_with = "double_option")]
    pub heater_emergency_absorb: Option<Option<bool>>,
    #[serde(default, deserialize_with = "double_option")]
    pub ambient_temp_c: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub base_load_kw: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub base_load_alpha: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub grid_import_limit_kw: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub grid_export_limit_kw: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub pv_generation_limit_kw: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub pv_plan_kw: Option<Option<f64>>,
}

/// Apply partial-merge: absent = no change, null = release (None), value = set.
fn merge_inject(current: &mut SimInjectState, body: PostSimInjectBody) {
    macro_rules! merge {
        ($field:ident) => {
            if let Some(v) = body.$field {
                current.$field = v;
            }
        };
    }
    merge!(battery_soc);
    merge!(ev_soc);
    merge!(heater_temp_c);
    merge!(pv_irradiance);
    if let Some(v) = body.pv_irradiance_alpha {
        current.pv_irradiance_alpha = v.unwrap_or(0.1); // null resets to default
    }
    merge!(ev_plugged);
    merge!(ev_soc_target);
    merge!(heater_setpoint_c);
    merge!(heater_temp_min_c);
    merge!(heater_temp_max_c);
    merge!(heater_emergency_curtail);
    merge!(heater_emergency_absorb);
    merge!(ambient_temp_c);
    merge!(base_load_kw);
    if let Some(v) = body.base_load_alpha {
        current.base_load_alpha = v.unwrap_or(0.1); // null resets to default
    }
    merge!(grid_import_limit_kw);
    merge!(grid_export_limit_kw);
    merge!(pv_generation_limit_kw);
    merge!(pv_plan_kw);
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
/// Reads the pre-computed schema from `AppCtx.sim_schema`. Does NOT acquire
/// the sim mutex, so it remains responsive even while the MILP planner is
/// running (10-24s on Node1).
pub async fn get_sim_schema(State(ctx): State<AppCtx>) -> impl IntoResponse {
    debug!("GET /sim/schema: returning pre-computed schema");
    let schema = (*ctx.sim_schema).clone();
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
///
/// Logged at `warn!` (not `debug!`) with the caller's source address: this endpoint has
/// caused multiple unattributed production incidents (repeated unexplained PV-irradiance
/// overrides on ven-1 — see docs/history/project_journal.md) because it previously left zero
/// trace of who called it. `ConnectInfo` only ever reflects the direct TCP peer, so a caller
/// behind the `ven-ui` nginx proxy shows up as nginx's address, not the original browser/script
/// — cross-reference nginx's own access log for that case.
pub async fn post_sim_inject(
    State(ctx): State<AppCtx>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<PostSimInjectBody>,
) -> impl IntoResponse {
    warn!(?peer, ?body, "POST /sim/inject");
    // Trigger a replan only for fields the MILP planner uses as inputs.
    // base_load_kw / base_load_alpha are one-shot physics overrides for test
    // simulation — triggering a replan on them would race the BDD assertion window
    // by adopting a new plan mid-test.
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
        || body.grid_export_limit_kw.is_some()
        || body.pv_generation_limit_kw.is_some();
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
pub async fn post_sim_inject_reset(
    State(ctx): State<AppCtx>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    warn!(?peer, "POST /sim/inject/reset");
    ctx.state.set_inject_state(SimInjectState::default()).await;
    axum::http::StatusCode::NO_CONTENT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_inject_sets_pv_generation_limit_kw() {
        let mut current = SimInjectState::default();
        let body = PostSimInjectBody {
            pv_generation_limit_kw: Some(Some(3.5)),
            ..Default::default()
        };
        merge_inject(&mut current, body);
        assert_eq!(current.pv_generation_limit_kw, Some(3.5));
    }

    #[test]
    fn merge_inject_null_releases_pv_generation_limit_kw() {
        let mut current = SimInjectState {
            pv_generation_limit_kw: Some(2.0),
            ..Default::default()
        };
        let body = PostSimInjectBody {
            pv_generation_limit_kw: Some(None),
            ..Default::default()
        };
        merge_inject(&mut current, body);
        assert_eq!(current.pv_generation_limit_kw, None);
    }

    #[test]
    fn merge_inject_absent_pv_generation_limit_kw_leaves_unchanged() {
        let mut current = SimInjectState {
            pv_generation_limit_kw: Some(2.0),
            ..Default::default()
        };
        let body = PostSimInjectBody::default();
        merge_inject(&mut current, body);
        assert_eq!(
            current.pv_generation_limit_kw,
            Some(2.0),
            "absent field must not change the current value"
        );
    }

    /// Regression: exercises the real `POST /sim/inject` deserialization path
    /// (`serde_json::from_str`, not a hand-built `PostSimInjectBody`). The tests
    /// above construct `PostSimInjectBody { field: Some(Value::Null), .. }`
    /// directly in Rust, which never proves a JSON `null` actually deserializes
    /// to that shape — `serde_json`'s blanket `Option<T>` impl collapses a
    /// top-level JSON `null` straight to Rust `None` for *any* `T`, including
    /// `T = serde_json::Value`, before `Value`'s own `Deserialize` ever runs.
    /// That makes `body.$field` `None` (indistinguishable from "absent") for an
    /// explicit `null`, so `merge_f64!`'s `if let Some(v) = body.$field` branch
    /// never fires and the field is silently never cleared — confirmed live on
    /// Node1 for both `pv_generation_limit_kw` and the pre-existing
    /// `grid_export_limit_kw`.
    #[test]
    fn post_body_null_actually_clears_via_real_json_deserialization() {
        let mut current = SimInjectState {
            pv_generation_limit_kw: Some(2.0),
            ..Default::default()
        };
        let body: PostSimInjectBody =
            serde_json::from_str(r#"{"pv_generation_limit_kw": null}"#).unwrap();
        merge_inject(&mut current, body);
        assert_eq!(
            current.pv_generation_limit_kw, None,
            "an explicit JSON null must clear the field when deserialized through \
             the real POST /sim/inject body type, not just when hand-constructed"
        );
    }
}
