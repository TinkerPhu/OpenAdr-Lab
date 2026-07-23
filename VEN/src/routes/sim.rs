use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use tracing::debug;

use crate::entities::asset::PlanTrigger;
use crate::entities::sim_inject::SimInjectState;
use crate::AppCtx;

/// serde_json's `Option<T>` deserializer treats a literal JSON `null` as `None`
/// directly — it never delegates to `T`, so a plain `Option<serde_json::Value>`
/// field can never actually observe an explicit `null` versus the key being
/// absent (both collapse to `None`). This "double option" wrapper is only
/// invoked by serde when the JSON key is present at all (an absent key skips
/// `deserialize_with` entirely and falls through to `#[serde(default)]` =
/// `None`), so the three states become distinguishable:
/// - Absent from JSON  → `None`       → no change to current state
/// - Present as `null` → `Some(None)` → release override
/// - Present as value  → `Some(Some(v))` → activate override with that value
fn double_option<'de, D, T>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Some(Option::deserialize(de)?))
}

/// Partial-merge body for POST /sim/inject. See `double_option` for the
/// absent/null/value serde semantics applied to every field below.
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
    pub ambient_temp_c: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub base_load_kw: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub base_load_alpha: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub pv_export_limit_kw: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub pv_plan_kw: Option<Option<f64>>,
}

/// Apply partial-merge: absent = no change, null = release (None), value = set.
fn merge_inject(current: &mut SimInjectState, body: PostSimInjectBody) {
    macro_rules! merge {
        ($field:ident) => {
            match body.$field {
                None => {}
                Some(None) => current.$field = None,
                Some(Some(v)) => current.$field = Some(v),
            }
        };
    }
    merge!(battery_soc);
    merge!(ev_soc);
    merge!(heater_temp_c);
    merge!(pv_irradiance);
    match body.pv_irradiance_alpha {
        None => {}
        Some(None) => current.pv_irradiance_alpha = 0.1, // reset to default
        Some(Some(alpha)) => current.pv_irradiance_alpha = alpha,
    }
    merge!(ev_plugged);
    merge!(ev_soc_target);
    merge!(heater_setpoint_c);
    merge!(heater_temp_min_c);
    merge!(heater_temp_max_c);
    merge!(ambient_temp_c);
    merge!(base_load_kw);
    match body.base_load_alpha {
        None => {}
        Some(None) => current.base_load_alpha = 0.1, // reset to default
        Some(Some(alpha)) => current.base_load_alpha = alpha,
    }
    merge!(pv_export_limit_kw);
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
/// running (10-24s on Pi4).
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

/// Trigger a replan only for fields the MILP planner uses as inputs.
/// base_load_kw / base_load_alpha are one-shot physics overrides for test
/// simulation — triggering a replan on them would race the BDD assertion window
/// by adopting a new plan mid-test.
/// pv_plan_kw is a planning-only forecast pin — it takes effect on the *next* scheduled
/// solve cycle and must NOT trigger an immediate replan, which would race the BDD
/// assertion window exactly like base_load_kw does.
fn body_triggers_replan(body: &PostSimInjectBody) -> bool {
    body.pv_irradiance.is_some()
        || body.battery_soc.is_some()
        || body.ev_soc.is_some()
        || body.ev_plugged.is_some()
        || body.ev_soc_target.is_some()
        || body.heater_temp_c.is_some()
        || body.heater_setpoint_c.is_some()
        || body.ambient_temp_c.is_some()
        || body.pv_export_limit_kw.is_some()
}

/// POST /sim/inject — partial-merge inject state.
/// Absent fields are unchanged; `null` releases the override; a value activates it.
pub async fn post_sim_inject(
    State(ctx): State<AppCtx>,
    Json(body): Json<PostSimInjectBody>,
) -> impl IntoResponse {
    let should_replan = body_triggers_replan(&body);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_body() -> PostSimInjectBody {
        PostSimInjectBody::default()
    }

    #[test]
    fn merge_inject_sets_pv_export_limit_kw_from_value() {
        let mut state = SimInjectState::default();
        let mut body = empty_body();
        body.pv_export_limit_kw = Some(Some(3.5));
        merge_inject(&mut state, body);
        assert_eq!(state.pv_export_limit_kw, Some(3.5));
    }

    #[test]
    fn merge_inject_clears_pv_export_limit_kw_on_null() {
        let mut state = SimInjectState {
            pv_export_limit_kw: Some(3.5),
            ..SimInjectState::default()
        };
        let mut body = empty_body();
        body.pv_export_limit_kw = Some(None);
        merge_inject(&mut state, body);
        assert_eq!(state.pv_export_limit_kw, None);
    }

    #[test]
    fn merge_inject_leaves_pv_export_limit_kw_unchanged_when_absent() {
        let mut state = SimInjectState {
            pv_export_limit_kw: Some(3.5),
            ..SimInjectState::default()
        };
        merge_inject(&mut state, empty_body());
        assert_eq!(state.pv_export_limit_kw, Some(3.5));
    }

    #[test]
    fn body_triggers_replan_true_when_pv_export_limit_kw_set() {
        let mut body = empty_body();
        body.pv_export_limit_kw = Some(Some(3.5));
        assert!(body_triggers_replan(&body));
    }

    #[test]
    fn body_triggers_replan_true_when_pv_export_limit_kw_cleared() {
        let mut body = empty_body();
        body.pv_export_limit_kw = Some(None);
        assert!(body_triggers_replan(&body));
    }

    #[test]
    fn body_triggers_replan_false_when_no_planner_input_fields_present() {
        assert!(!body_triggers_replan(&empty_body()));
    }

    // ── Regression coverage for the real JSON-deserialization boundary ──────
    // The bug this guards against: serde_json's `Option<T>` deserializer
    // treats a literal JSON `null` as `None` directly, never delegating to
    // `T` — so a naive `Option<serde_json::Value>` field can never actually
    // observe `null` distinctly from the key being absent. Tests that only
    // construct `PostSimInjectBody` fields directly in Rust (like the ones
    // above) don't exercise that boundary at all and would pass even if this
    // were broken — confirmed live on Pi4 (`pv_irradiance` and
    // `pv_export_limit_kw` both failed to clear via real `POST /sim/inject`
    // before the `double_option` fix). These tests deserialize actual JSON.

    #[test]
    fn json_absent_key_deserializes_to_outer_none() {
        let body: PostSimInjectBody = serde_json::from_str("{}").unwrap();
        assert_eq!(body.pv_export_limit_kw, None);
    }

    #[test]
    fn json_explicit_null_deserializes_to_some_none() {
        let body: PostSimInjectBody =
            serde_json::from_str(r#"{"pv_export_limit_kw": null}"#).unwrap();
        assert_eq!(body.pv_export_limit_kw, Some(None));
    }

    #[test]
    fn json_value_deserializes_to_some_some() {
        let body: PostSimInjectBody =
            serde_json::from_str(r#"{"pv_export_limit_kw": 3.5}"#).unwrap();
        assert_eq!(body.pv_export_limit_kw, Some(Some(3.5)));
    }

    #[test]
    fn json_null_round_trip_actually_clears_the_field() {
        // End-to-end through the real deserialization boundary AND merge_inject.
        let mut state = SimInjectState {
            pv_export_limit_kw: Some(3.5),
            ..SimInjectState::default()
        };
        let body: PostSimInjectBody =
            serde_json::from_str(r#"{"pv_export_limit_kw": null}"#).unwrap();
        merge_inject(&mut state, body);
        assert_eq!(
            state.pv_export_limit_kw, None,
            "a JSON null in the real request body must clear the field"
        );
    }

    #[test]
    fn json_bool_field_null_round_trip_actually_clears() {
        let mut state = SimInjectState {
            ev_plugged: Some(true),
            ..SimInjectState::default()
        };
        let body: PostSimInjectBody = serde_json::from_str(r#"{"ev_plugged": null}"#).unwrap();
        merge_inject(&mut state, body);
        assert_eq!(state.ev_plugged, None);
    }
}
