use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};

use crate::error::AppError;
use crate::routes::request_id;
use crate::AppCtx;

pub async fn get_vens(
    State(ctx): State<AppCtx>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let rid = request_id(&headers);
    let data = crate::routes::cached_collection(
        &ctx.business,
        &ctx,
        "vens",
        "/vens",
        ctx.config.cache_ttl_vens,
        rid.as_deref(),
    )
    .await?;
    Ok(Json(data))
}

pub async fn delete_ven(
    State(ctx): State<AppCtx>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let rid = request_id(&headers);
    ctx.business
        .delete_json(&format!("/vens/{id}"), rid.as_deref())
        .await?;
    ctx.cache.invalidate("vens").await;
    Ok(Json(serde_json::json!({"deleted": id})))
}
