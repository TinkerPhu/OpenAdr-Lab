use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use std::time::Duration;

use crate::error::AppError;
use crate::routes::request_id;
use crate::AppCtx;

pub async fn get_reports(
    State(ctx): State<AppCtx>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    if let Some(cached) = ctx.cache.get("reports").await {
        return Ok(Json(cached));
    }

    let rid = request_id(&headers);
    let data = ctx.business.get_json("/reports", rid.as_deref()).await?;
    ctx.cache
        .set(
            "reports".into(),
            data.clone(),
            Duration::from_secs(ctx.config.cache_ttl_reports),
        )
        .await;
    Ok(Json(data))
}

pub async fn delete_report(
    State(ctx): State<AppCtx>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let rid = request_id(&headers);
    ctx.business.delete_json(&format!("/reports/{id}"), rid.as_deref()).await?;
    ctx.cache.invalidate("reports").await;
    Ok(Json(serde_json::json!({"deleted": id})))
}
