//! `DomainError` → HTTP, the project's one presentation-boundary translation
//! for domain failures (`docs/guidelines/ERROR_HANDLING.md`).
//!
//! It used to live in `services/hems.rs`, next to a dead `HvacService` and
//! under a module name that said nothing about HTTP — which is why deleting
//! that service was briefly mistaken for deleting only dead code. Choosing a
//! status code is a presentation decision, so it belongs in `routes/`, where
//! a reader looking for "what does a `NotFound` become on the wire" would
//! look for it.

use axum::http::StatusCode;
use axum::Json;

use crate::entities::DomainError;

impl From<DomainError> for (StatusCode, Json<serde_json::Value>) {
    fn from(e: DomainError) -> Self {
        let status = match e {
            DomainError::NotFound { .. } | DomainError::AssetNotFound { .. } => {
                StatusCode::NOT_FOUND
            }
            DomainError::InvalidValue { .. } => StatusCode::BAD_REQUEST,
            DomainError::SessionConflict(_) => StatusCode::CONFLICT,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(serde_json::json!({ "error": e.to_string() })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status_of(e: DomainError) -> StatusCode {
        let (status, _): (StatusCode, Json<serde_json::Value>) = e.into();
        status
    }

    #[test]
    fn not_found_maps_to_404() {
        assert_eq!(
            status_of(DomainError::NotFound {
                id: uuid::Uuid::new_v4()
            }),
            StatusCode::NOT_FOUND
        );
    }

    #[test]
    fn an_unknown_asset_maps_to_404_and_a_refused_value_to_400() {
        assert_eq!(
            status_of(DomainError::AssetNotFound {
                asset_id: "nope".into()
            }),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            status_of(DomainError::InvalidValue {
                asset_id: "battery".into(),
                key: "soc".into(),
                message: "soc must be between 0.0 and 1.0".into(),
            }),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn session_conflict_maps_to_409() {
        assert_eq!(
            status_of(DomainError::SessionConflict("ev busy".to_string())),
            StatusCode::CONFLICT
        );
    }

    #[test]
    fn the_body_carries_the_errors_own_display() {
        let e = DomainError::SessionConflict("ev busy".to_string());
        let expected = e.to_string();
        let (_, Json(body)): (StatusCode, Json<serde_json::Value>) = e.into();
        assert_eq!(body["error"], expected);
    }
}
