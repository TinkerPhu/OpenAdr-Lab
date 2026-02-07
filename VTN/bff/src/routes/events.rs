use axum::{
    extract::{Path, State},
    Json,
};
use std::time::Duration;

use crate::error::AppError;
use crate::AppCtx;

pub async fn get_events(State(ctx): State<AppCtx>) -> Result<Json<serde_json::Value>, AppError> {
    if let Some(cached) = ctx.cache.get("events").await {
        return Ok(Json(cached));
    }

    let data = ctx.business.get_json("/events").await?;
    ctx.cache
        .set(
            "events".into(),
            data.clone(),
            Duration::from_secs(ctx.config.cache_ttl_events),
        )
        .await;
    Ok(Json(data))
}

pub async fn create_event(
    State(ctx): State<AppCtx>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let data = ctx.business.post_json("/events", body).await?;
    ctx.cache.invalidate("events").await;
    Ok(Json(data))
}

pub async fn update_event(
    State(ctx): State<AppCtx>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let data = ctx.business.put_json(&format!("/events/{id}"), body).await?;
    ctx.cache.invalidate("events").await;
    Ok(Json(data))
}

pub async fn delete_event(
    State(ctx): State<AppCtx>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    ctx.business.delete_json(&format!("/events/{id}")).await?;
    ctx.cache.invalidate("events").await;
    Ok(Json(serde_json::json!({"deleted": id})))
}
