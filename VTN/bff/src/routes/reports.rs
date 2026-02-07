use axum::{
    extract::{Path, State},
    Json,
};
use std::time::Duration;

use crate::error::AppError;
use crate::AppCtx;

pub async fn get_reports(State(ctx): State<AppCtx>) -> Result<Json<serde_json::Value>, AppError> {
    if let Some(cached) = ctx.cache.get("reports").await {
        return Ok(Json(cached));
    }

    let data = ctx.business.get_json("/reports").await?;
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
) -> Result<Json<serde_json::Value>, AppError> {
    ctx.business.delete_json(&format!("/reports/{id}")).await?;
    ctx.cache.invalidate("reports").await;
    Ok(Json(serde_json::json!({"deleted": id})))
}
