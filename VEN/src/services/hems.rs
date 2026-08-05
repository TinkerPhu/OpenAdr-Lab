use crate::entities::device_session::HeaterTarget;
use crate::entities::DomainError;
use crate::state::AppState;

// BL-41: heater target is now set/cleared exclusively through the unified /user-requests
// flow (services::user_request::UserRequestService, state.cancel_request handles session
// clearing atomically) — the direct-CRUD route this comment used to reference (post_heater_target)
// is gone. Kept intentionally (not deleted) — see docs/BACKLOG.md BL-23 for the removal decision.
#[allow(dead_code)]
pub struct HvacService;

#[allow(dead_code)]
impl HvacService {
    pub async fn set_heater_target(target: HeaterTarget, state: &AppState) {
        state.set_heater_target(Some(target)).await;
    }

    pub async fn clear_heater_target(state: &AppState) {
        state.set_heater_target(None).await;
    }
}

/// Shared DomainError → HTTP response mapping, used by routes/hems/sessions.rs's
/// delete_request (and previously also by the now-removed direct-CRUD device-session
/// routes, BL-41).
impl From<DomainError> for (axum::http::StatusCode, axum::Json<serde_json::Value>) {
    fn from(e: DomainError) -> Self {
        use axum::http::StatusCode;
        match e {
            DomainError::NotFound { .. } => (
                StatusCode::NOT_FOUND,
                axum::Json(serde_json::json!({"error": e.to_string()})),
            ),
            DomainError::SessionConflict(_) => (
                StatusCode::CONFLICT,
                axum::Json(serde_json::json!({"error": e.to_string()})),
            ),
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({"error": e.to_string()})),
            ),
        }
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_heater_clear_removes_target() {
        let state = AppState::new();
        let target = HeaterTarget {
            mode: Default::default(),
            id: Uuid::new_v4(),
            target_temp_c: 55.0,
            ready_by: Utc::now() + chrono::Duration::hours(2),
            comfort_rates: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        HvacService::set_heater_target(target, &state).await;
        assert!(state.heater_target().await.is_some());

        HvacService::clear_heater_target(&state).await;
        assert!(
            state.heater_target().await.is_none(),
            "heater target must be cleared"
        );
    }
}
