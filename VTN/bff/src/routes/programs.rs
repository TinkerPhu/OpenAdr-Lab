use axum::{
    extract::{Path, State},
    Json,
};
use std::time::Duration;

use crate::error::AppError;
use crate::AppCtx;

pub async fn get_programs(State(ctx): State<AppCtx>) -> Result<Json<serde_json::Value>, AppError> {
    if let Some(cached) = ctx.cache.get("programs").await {
        return Ok(Json(cached));
    }

    let data = ctx.business.get_json("/programs").await?;
    ctx.cache
        .set(
            "programs".into(),
            data.clone(),
            Duration::from_secs(ctx.config.cache_ttl_programs),
        )
        .await;
    Ok(Json(data))
}

pub async fn create_program(
    State(ctx): State<AppCtx>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let data = ctx.business.post_json("/programs", body).await?;
    ctx.cache.invalidate("programs").await;
    Ok(Json(data))
}

pub async fn update_program(
    State(ctx): State<AppCtx>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let data = ctx.business.put_json(&format!("/programs/{id}"), body).await?;
    ctx.cache.invalidate("programs").await;
    Ok(Json(data))
}

pub async fn delete_program(
    State(ctx): State<AppCtx>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    ctx.business.delete_json(&format!("/programs/{id}")).await?;
    ctx.cache.invalidate("programs").await;
    Ok(Json(serde_json::json!({"deleted": id})))
}
