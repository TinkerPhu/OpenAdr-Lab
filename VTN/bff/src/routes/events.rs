use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use std::time::Duration;

use crate::error::AppError;
use crate::routes::request_id;
use crate::AppCtx;

pub async fn get_events(
    State(ctx): State<AppCtx>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    if let Some(cached) = ctx.cache.get("events").await {
        return Ok(Json(cached));
    }

    let rid = request_id(&headers);
    let data = ctx.business.get_json("/events", rid.as_deref()).await?;
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
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let rid = request_id(&headers);
    let data = ctx.business.post_json("/events", body, rid.as_deref()).await?;
    ctx.cache.invalidate("events").await;
    Ok(Json(data))
}

pub async fn update_event(
    State(ctx): State<AppCtx>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let rid = request_id(&headers);
    let data = ctx.business.put_json(&format!("/events/{id}"), body, rid.as_deref()).await?;
    ctx.cache.invalidate("events").await;
    Ok(Json(data))
}

pub async fn delete_event(
    State(ctx): State<AppCtx>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let rid = request_id(&headers);
    ctx.business.delete_json(&format!("/events/{id}"), rid.as_deref()).await?;
    ctx.cache.invalidate("events").await;
    Ok(Json(serde_json::json!({"deleted": id})))
}
