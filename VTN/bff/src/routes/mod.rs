pub mod events;
pub mod health;
pub mod metrics;
pub mod programs;
pub mod reports;
pub mod vens;

use axum::http::HeaderMap;
use std::time::Duration;

use crate::error::AppError;
use crate::vtn_client::VtnClient;

/// Extract the X-Request-ID header value as an owned String.
pub fn request_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_owned())
}

/// Serve a VTN collection: cache hit, else fetch **every page** and cache the
/// whole list under `cache_key`.
///
/// The single "list a VTN collection for the UI" path (R-84) — every list
/// route goes through here, so none of them can fall back to a one-page
/// `get_json` and silently drop everything past the VTN's 50-row page.
pub(crate) async fn cached_collection(
    client: &VtnClient,
    ctx: &crate::AppCtx,
    cache_key: &str,
    path: &str,
    ttl_secs: u64,
    request_id: Option<&str>,
) -> Result<serde_json::Value, AppError> {
    if let Some(cached) = ctx.cache.get(cache_key).await {
        return Ok(cached);
    }

    let rows = client.get_all_pages(path, request_id).await?;
    let data = serde_json::Value::Array(rows);
    ctx.cache
        .set(
            cache_key.into(),
            data.clone(),
            Duration::from_secs(ttl_secs),
        )
        .await;
    Ok(data)
}
