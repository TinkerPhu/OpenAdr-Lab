use axum::{http::StatusCode, response::IntoResponse, Json};
use serde_json::json;

/// A VTN response with a known upstream status, carried through `anyhow::Error` so
/// `AppError` can recover the real status class instead of flattening to 502.
#[derive(Debug)]
pub struct UpstreamStatusError {
    pub status: StatusCode,
    pub message: String,
}

impl std::fmt::Display for UpstreamStatusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for UpstreamStatusError {}

pub struct AppError(pub anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        tracing::error!("{:#}", self.0);

        let (status, message) = match self.0.downcast_ref::<UpstreamStatusError>() {
            Some(err) if err.status.is_client_error() => (err.status, err.message.clone()),
            _ => (StatusCode::BAD_GATEWAY, self.0.to_string()),
        };

        (status, Json(json!({ "error": message }))).into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Untyped errors (network failures, JSON parse errors, token failures) still
    // flatten to 502 — there is no known upstream status to propagate.
    #[tokio::test]
    async fn into_response_maps_untyped_error_to_502_with_json_error_body() {
        let err = AppError(anyhow::anyhow!("upstream exploded"));
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);

        let bytes = axum::body::to_bytes(resp.into_body(), 1024)
            .await
            .expect("body must be readable");
        let body: serde_json::Value = serde_json::from_slice(&bytes).expect("body must be JSON");
        assert_eq!(body["error"], "upstream exploded");
    }

    // A `UpstreamStatusError` with a known 4xx status propagates that exact status
    // instead of flattening to 502 — this is the R-31 fix.
    #[tokio::test]
    async fn into_response_propagates_known_upstream_4xx_status() {
        let err = AppError(
            UpstreamStatusError {
                status: StatusCode::CONFLICT,
                message: "/programs/p1 returned 409 Conflict: name already exists".into(),
            }
            .into(),
        );
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::CONFLICT);

        let bytes = axum::body::to_bytes(resp.into_body(), 1024)
            .await
            .expect("body must be readable");
        let body: serde_json::Value = serde_json::from_slice(&bytes).expect("body must be JSON");
        assert_eq!(
            body["error"],
            "/programs/p1 returned 409 Conflict: name already exists"
        );
    }

    // A `UpstreamStatusError` with a 5xx status still flattens to 502 — a genuine
    // upstream server failure, not a client-facing validation/conflict error.
    #[tokio::test]
    async fn into_response_maps_upstream_5xx_status_to_502() {
        let err = AppError(
            UpstreamStatusError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "/programs returned 500: boom".into(),
            }
            .into(),
        );
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
    }

    #[test]
    fn from_converts_any_error_into_app_error() {
        let io_err = std::io::Error::other("disk gone");
        let app_err: AppError = io_err.into();
        assert_eq!(app_err.0.to_string(), "disk gone");
    }
}
