use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    Json,
};
use serde::Deserialize;
use std::time::Duration;

use crate::error::AppError;
use crate::routes::request_id;
use crate::AppCtx;

#[derive(Deserialize)]
pub struct EventsQuery {
    pub active: Option<bool>,
}

pub async fn get_events(
    State(ctx): State<AppCtx>,
    Query(query): Query<EventsQuery>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let cache_key = match query.active {
        Some(true) => "events?active=true",
        Some(false) => "events?active=false",
        None => "events",
    };

    if let Some(cached) = ctx.cache.get(cache_key).await {
        return Ok(Json(cached));
    }

    let path = match query.active {
        Some(val) => format!("/events?active={val}"),
        None => "/events".to_string(),
    };

    let rid = request_id(&headers);
    let data = ctx.business.get_json(&path, rid.as_deref()).await?;
    ctx.cache
        .set(
            cache_key.into(),
            data.clone(),
            Duration::from_secs(ctx.config.cache_ttl_events),
        )
        .await;
    Ok(Json(data))
}

async fn invalidate_events_cache(ctx: &AppCtx) {
    ctx.cache.invalidate("events").await;
    ctx.cache.invalidate("events?active=true").await;
    ctx.cache.invalidate("events?active=false").await;
}

pub async fn create_event(
    State(ctx): State<AppCtx>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let rid = request_id(&headers);
    let data = ctx.business.post_json("/events", body, rid.as_deref()).await?;
    invalidate_events_cache(&ctx).await;
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
    invalidate_events_cache(&ctx).await;
    Ok(Json(data))
}

pub async fn delete_event(
    State(ctx): State<AppCtx>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let rid = request_id(&headers);
    ctx.business.delete_json(&format!("/events/{id}"), rid.as_deref()).await?;
    invalidate_events_cache(&ctx).await;
    Ok(Json(serde_json::json!({"deleted": id})))
}
