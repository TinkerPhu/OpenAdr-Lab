use axum::{extract::State, Json};
use std::time::Duration;

use crate::error::AppError;
use crate::AppCtx;

pub async fn get_events(State(ctx): State<AppCtx>) -> Result<Json<serde_json::Value>, AppError> {
    if let Some(cached) = ctx.cache.get("events").await {
        return Ok(Json(cached));
    }

    let data = ctx.vtn.get_json("/events").await?;
    ctx.cache
        .set(
            "events".into(),
            data.clone(),
            Duration::from_secs(ctx.config.cache_ttl_events),
        )
        .await;
    Ok(Json(data))
}
