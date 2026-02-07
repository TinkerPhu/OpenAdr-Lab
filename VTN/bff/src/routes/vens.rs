use axum::{
    extract::{Path, State},
    Json,
};
use std::time::Duration;

use crate::error::AppError;
use crate::AppCtx;

pub async fn get_vens(State(ctx): State<AppCtx>) -> Result<Json<serde_json::Value>, AppError> {
    if let Some(cached) = ctx.cache.get("vens").await {
        return Ok(Json(cached));
    }

    let data = ctx.ven_mgr.get_json("/vens").await?;
    ctx.cache
        .set(
            "vens".into(),
            data.clone(),
            Duration::from_secs(ctx.config.cache_ttl_vens),
        )
        .await;
    Ok(Json(data))
}

pub async fn delete_ven(
    State(ctx): State<AppCtx>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    ctx.ven_mgr.delete_json(&format!("/vens/{id}")).await?;
    ctx.cache.invalidate("vens").await;
    Ok(Json(serde_json::json!({"deleted": id})))
}
