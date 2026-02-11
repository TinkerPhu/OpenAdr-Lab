pub mod events;
pub mod health;
pub mod metrics;
pub mod programs;
pub mod reports;
pub mod vens;

use axum::http::HeaderMap;

/// Extract the X-Request-ID header value as an owned String.
pub fn request_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_owned())
}
