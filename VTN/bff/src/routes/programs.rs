use axum::{extract::State, Json};
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
