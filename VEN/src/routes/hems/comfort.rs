//! WP4.2 (BL-19) — user comfort-curve override routes.
//!
//! `GET    /assets/:id/comfort_curve` — effective curve + its source
//! `POST   /assets/:id/comfort_curve` — install an override (validated)
//! `DELETE /assets/:id/comfort_curve` — restore the built-in default
//!
//! Wire shape is the domain `ComfortRate` unchanged (DTO passthrough).

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use tracing::info;

use serde::Serialize;
use ts_rs::TS;

use crate::app_ctx::{Settings, SimRead};
use crate::entities::asset::ComfortRate;
use crate::entities::comfort::ComfortCurveSource;
use crate::entities::comfort_spend::{rates_from_cumulative_spend, ComfortSpendPoint};
use crate::state::AppState;

/// `GET` / `POST /assets/:id/comfort_curve`: the effective curve and where it comes from.
#[derive(Debug, Clone, Serialize, TS)]
pub struct ComfortCurveResponse {
    pub source: ComfortCurveSource,
    pub rates: Vec<ComfortRate>,
}

/// Default curve for an asset, `None` when the asset id is unknown.
async fn default_rates(
    sim_read: &dyn crate::controller::SimReadPort,
    asset_id: &str,
) -> Option<Vec<ComfortRate>> {
    sim_read.default_comfort_rates(asset_id).await
}

/// GET /assets/:id/comfort_curve
pub async fn get_comfort_curve(
    State(sim_read): State<SimRead>,
    State(state): State<AppState>,
    Path(asset_id): Path<String>,
) -> impl IntoResponse {
    let Some(default) = default_rates(sim_read.as_ref(), &asset_id).await else {
        return crate::routes::error::unknown_asset(&asset_id);
    };
    let overrides = state.comfort_overrides_map().await;
    let source = crate::entities::comfort::comfort_curve_source(&overrides, &asset_id);
    let rates = crate::entities::comfort::effective_comfort_rates(&overrides, &asset_id, default);
    Json(ComfortCurveResponse { source, rates }).into_response()
}

/// POST /assets/:id/comfort_curve/cost: what the given curve can cost for the energy ahead,
/// the same curve as cumulative spend, and what the active plan spends on the asset. A preview:
/// nothing is stored, so the card can show it for rows that are not saved yet.
pub async fn post_comfort_curve_cost(
    State(sim_read): State<SimRead>,
    State(state): State<AppState>,
    Path(asset_id): Path<String>,
    Json(rates): Json<Vec<ComfortRate>>,
) -> impl IntoResponse {
    if default_rates(sim_read.as_ref(), &asset_id).await.is_none() {
        return crate::routes::error::unknown_asset(&asset_id);
    }
    if let Err(e) = crate::entities::comfort::validate_curve(&rates) {
        return crate::routes::error::error_response(StatusCode::UNPROCESSABLE_ENTITY, e);
    }
    let bands = sim_read.comfort_bands(&asset_id, &rates).await;
    let span = sim_read.comfort_fill_span(&asset_id).await;
    let plan = state.active_plan().await;
    Json(crate::entities::comfort::comfort_curve_cost(
        &crate::entities::comfort::ComfortCurveBasis {
            rates: &rates,
            bands: bands.as_deref(),
            span: span.as_ref(),
        },
        plan.as_ref(),
        &asset_id,
    ))
    .into_response()
}

/// POST /assets/:id/comfort_curve/spend: install a curve given as cumulative spend from the
/// asset's current charge. It becomes the marginal curve the planner reads and is stored like any
/// override; an asset that declares no energy per fill is refused.
pub async fn post_comfort_curve_spend(
    State(sim_read): State<SimRead>,
    State(state): State<AppState>,
    State(settings): State<Settings>,
    Path(asset_id): Path<String>,
    Json(points): Json<Vec<ComfortSpendPoint>>,
) -> impl IntoResponse {
    if default_rates(sim_read.as_ref(), &asset_id).await.is_none() {
        return crate::routes::error::unknown_asset(&asset_id);
    }
    let Some(span) = sim_read.comfort_fill_span(&asset_id).await else {
        return crate::routes::error::error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("spend input is not available for asset {asset_id}"),
        );
    };
    match rates_from_cumulative_spend(&points, &span) {
        Ok(rates) => install_override(&state, settings, &asset_id, rates).await,
        Err(e) => crate::routes::error::error_response(StatusCode::UNPROCESSABLE_ENTITY, e),
    }
}

/// Validate, store and persist a curve as the asset's override: the answer both POST routes give.
async fn install_override(
    state: &AppState,
    settings: Option<std::sync::Arc<dyn crate::controller::SettingsPort>>,
    asset_id: &str,
    rates: Vec<ComfortRate>,
) -> axum::response::Response {
    match crate::services::comfort::set_override(
        state,
        settings,
        Utc::now(),
        asset_id,
        rates.clone(),
    )
    .await
    {
        Ok(()) => {
            info!(asset_id, points = rates.len(), "comfort curve override set");
            (
                StatusCode::CREATED,
                Json(ComfortCurveResponse {
                    source: ComfortCurveSource::Override,
                    rates,
                }),
            )
                .into_response()
        }
        Err(e) => crate::routes::error::error_response(StatusCode::UNPROCESSABLE_ENTITY, e),
    }
}

/// POST /assets/:id/comfort_curve
pub async fn post_comfort_curve(
    State(sim_read): State<SimRead>,
    State(state): State<AppState>,
    State(settings): State<Settings>,
    Path(asset_id): Path<String>,
    Json(rates): Json<Vec<ComfortRate>>,
) -> impl IntoResponse {
    if default_rates(sim_read.as_ref(), &asset_id).await.is_none() {
        return crate::routes::error::unknown_asset(&asset_id);
    }
    install_override(&state, settings.clone(), &asset_id, rates).await
}

/// DELETE /assets/:id/comfort_curve
pub async fn delete_comfort_curve(
    State(state): State<AppState>,
    State(settings): State<Settings>,
    Path(asset_id): Path<String>,
) -> impl IntoResponse {
    let existed =
        crate::services::comfort::clear_override(&state, settings.clone(), &asset_id).await;
    info!(asset_id, existed, "comfort curve override cleared");
    if existed {
        StatusCode::NO_CONTENT.into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R-133: the typed body is the `json!()` it replaced, key for key.
    #[test]
    fn comfort_curve_response_keeps_the_wire_it_replaced() {
        let rates = vec![crate::entities::comfort::curve_point(0.5, 0.2)];
        let body = ComfortCurveResponse {
            source: ComfortCurveSource::Override,
            rates: rates.clone(),
        };
        assert_eq!(
            serde_json::to_value(&body).unwrap(),
            serde_json::json!({ "source": "override", "rates": rates })
        );
    }
}
