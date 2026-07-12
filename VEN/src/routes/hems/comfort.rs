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

use crate::entities::asset::ComfortRate;
use crate::AppCtx;

/// Default curve for an asset, `None` when the asset id is unknown.
async fn default_rates(ctx: &AppCtx, asset_id: &str) -> Option<Vec<ComfortRate>> {
    let sim = ctx.sim.lock().await;
    sim.find_asset(asset_id)
        .map(|(_, cfg)| cfg.default_comfort_rates())
}

/// GET /assets/:id/comfort_curve
pub async fn get_comfort_curve(
    State(ctx): State<AppCtx>,
    Path(asset_id): Path<String>,
) -> impl IntoResponse {
    let Some(default) = default_rates(&ctx, &asset_id).await else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("unknown asset: {asset_id}") })),
        )
            .into_response();
    };
    let overrides = ctx.state.comfort_overrides_map().await;
    let source = if overrides.contains_key(&asset_id) {
        "override"
    } else {
        "default"
    };
    let rates = crate::services::comfort::effective_comfort_rates(&overrides, &asset_id, default);
    Json(serde_json::json!({ "source": source, "rates": rates })).into_response()
}

/// POST /assets/:id/comfort_curve
pub async fn post_comfort_curve(
    State(ctx): State<AppCtx>,
    Path(asset_id): Path<String>,
    Json(rates): Json<Vec<ComfortRate>>,
) -> impl IntoResponse {
    if default_rates(&ctx, &asset_id).await.is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("unknown asset: {asset_id}") })),
        )
            .into_response();
    }
    match crate::services::comfort::set_override(
        &ctx.state,
        ctx.settings.clone(),
        Utc::now(),
        &asset_id,
        rates.clone(),
    )
    .await
    {
        Ok(()) => {
            info!(asset_id, points = rates.len(), "comfort curve override set");
            (
                StatusCode::CREATED,
                Json(serde_json::json!({ "source": "override", "rates": rates })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}

/// DELETE /assets/:id/comfort_curve
pub async fn delete_comfort_curve(
    State(ctx): State<AppCtx>,
    Path(asset_id): Path<String>,
) -> impl IntoResponse {
    let existed =
        crate::services::comfort::clear_override(&ctx.state, ctx.settings.clone(), &asset_id).await;
    info!(asset_id, existed, "comfort curve override cleared");
    if existed {
        StatusCode::NO_CONTENT.into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}
