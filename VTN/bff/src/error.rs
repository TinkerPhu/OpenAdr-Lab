use axum::{http::StatusCode, response::IntoResponse, Json};
use serde_json::json;

pub struct AppError(pub anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        tracing::error!("{:#}", self.0);
        (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": self.0.to_string() })),
        )
            .into_response()
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

    // Documents the current contract: every AppError maps to 502 with a JSON
    // `error` field. Upstream 4xx flattening to 502 is tracked as R-31.
    #[tokio::test]
    async fn into_response_maps_to_502_with_json_error_body() {
        let err = AppError(anyhow::anyhow!("upstream exploded"));
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);

        let bytes = axum::body::to_bytes(resp.into_body(), 1024)
            .await
            .expect("body must be readable");
        let body: serde_json::Value = serde_json::from_slice(&bytes).expect("body must be JSON");
        assert_eq!(body["error"], "upstream exploded");
    }

    #[test]
    fn from_converts_any_error_into_app_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::Other, "disk gone");
        let app_err: AppError = io_err.into();
        assert_eq!(app_err.0.to_string(), "disk gone");
    }
}
