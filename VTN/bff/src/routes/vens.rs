use axum::{extract::State, Json};
use std::time::Duration;

use crate::error::AppError;
use crate::AppCtx;

pub async fn get_vens(State(ctx): State<AppCtx>) -> Result<Json<serde_json::Value>, AppError> {
    if let Some(cached) = ctx.cache.get("vens").await {
        return Ok(Json(cached));
    }

    let data = ctx.ven_mgr.get_json("/vens").await?;
    ctx.cache
        .set(
            "vens".into(),
            data.clone(),
            Duration::from_secs(ctx.config.cache_ttl_vens),
        )
        .await;
    Ok(Json(data))
}
