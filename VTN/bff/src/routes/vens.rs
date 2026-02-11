use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use std::time::Duration;

use crate::error::AppError;
use crate::routes::request_id;
use crate::AppCtx;

pub async fn get_vens(
    State(ctx): State<AppCtx>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    if let Some(cached) = ctx.cache.get("vens").await {
        return Ok(Json(cached));
    }

    let rid = request_id(&headers);
    let data = ctx.ven_mgr.get_json("/vens", rid.as_deref()).await?;
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
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let rid = request_id(&headers);
    ctx.ven_mgr.delete_json(&format!("/vens/{id}"), rid.as_deref()).await?;
    ctx.cache.invalidate("vens").await;
    Ok(Json(serde_json::json!({"deleted": id})))
}
