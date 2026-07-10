use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use std::time::Duration;

use crate::error::AppError;
use crate::routes::request_id;
use crate::AppCtx;

pub async fn get_programs(
    State(ctx): State<AppCtx>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    if let Some(cached) = ctx.cache.get("programs").await {
        return Ok(Json(cached));
    }

    let rid = request_id(&headers);
    let data = ctx.business.get_json("/programs", rid.as_deref()).await?;
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
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let rid = request_id(&headers);
    let data = ctx
        .business
        .post_json("/programs", body, rid.as_deref())
        .await?;
    ctx.cache.invalidate("programs").await;
    Ok(Json(data))
}

pub async fn update_program(
    State(ctx): State<AppCtx>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let rid = request_id(&headers);
    let data = ctx
        .business
        .put_json(&format!("/programs/{id}"), body, rid.as_deref())
        .await?;
    ctx.cache.invalidate("programs").await;
    Ok(Json(data))
}

pub async fn delete_program(
    State(ctx): State<AppCtx>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let rid = request_id(&headers);
    ctx.business
        .delete_json(&format!("/programs/{id}"), rid.as_deref())
        .await?;
    ctx.cache.invalidate("programs").await;
    Ok(Json(serde_json::json!({"deleted": id})))
}
