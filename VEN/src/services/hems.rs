use chrono::Utc;
use tracing::info;
use uuid::Uuid;

use crate::entities::device_session::{EvSession, HeaterTarget};
use crate::entities::user_request::UserRequestStatus;
use crate::entities::DomainError;
use crate::state::AppState;

pub struct EvSessionService;
// Not yet wired to route; heater target is set directly in post_heater_target.
#[allow(dead_code)]
pub struct HvacService;

impl EvSessionService {
    /// Start a new EV session. Returns `SessionConflict` if one is already active.
    pub async fn start(session: EvSession, state: &AppState) -> Result<(), DomainError> {
        if state.ev_session().await.is_some() {
            return Err(DomainError::SessionConflict(
                "an EV session is already active; delete it before creating a new one".into(),
            ));
        }
        state.set_ev_session(Some(session)).await;
        Ok(())
    }

    /// End the active EV session: clears it from state and transitions any linked
    /// active UserRequest to Completed.
    pub async fn end(state: &AppState) -> Result<(), DomainError> {
        let session = state.ev_session().await;
        if session.is_none() {
            return Err(DomainError::NotFound { id: Uuid::nil() });
        }
        let session_id = session.unwrap().id;

        // Transition any linked active request to Completed.
        let mut requests = state.active_requests().await;
        for req in requests.iter_mut() {
            if req.session_id == Some(session_id) && req.status == UserRequestStatus::Active {
                req.status = UserRequestStatus::Completed;
                req.updated_at = Utc::now();
                info!(request_id = %req.id, "user request completed (EV session ended)");
            }
        }
        state.set_active_requests(requests).await;
        state.set_ev_session(None).await;

        Ok(())
    }
}

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

#[allow(dead_code)]
impl HvacService {
    pub async fn set_heater_target(target: HeaterTarget, state: &AppState) {
        state.set_heater_target(Some(target)).await;
    }

    pub async fn clear_heater_target(state: &AppState) {
        state.set_heater_target(None).await;
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::device_session::EvSession;
    use crate::entities::user_request::{UserRequest, UserRequestStatus};
    use crate::entities::DomainError;
    use crate::state::AppState;
    use chrono::Utc;
    use uuid::Uuid;

    fn make_ev_session() -> EvSession {
        EvSession {
            id: Uuid::new_v4(),
            target_soc: 0.8,
            departure_time: Utc::now() + chrono::Duration::hours(6),
            soft_deadline: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn make_active_request(session_id: Uuid) -> UserRequest {
        UserRequest {
            id: Uuid::new_v4(),
            asset_id: "ev".to_string(),
            status: UserRequestStatus::Active,
            target_soc: Some(0.8),
            target_energy_kwh: 10.0,
            desired_power_kw: 3.0,
            deadlines: vec![],
            completion_policy: "STOP".to_string(),
            max_total_cost_eur: None,
            tier_count: 0,
            session_id: Some(session_id),
            session_type: Some(crate::entities::user_request::SessionType::Ev),
            estimated_cost_eur: 0.0,
            estimated_co2_g: 0.0,
            interruptible: false,
            tolerance_min: None,
            budget_eur: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_ev_session_end_clears_session() {
        let state = AppState::new();
        let session = make_ev_session();
        state.set_ev_session(Some(session)).await;

        EvSessionService::end(&state)
            .await
            .expect("end must succeed");

        assert!(
            state.ev_session().await.is_none(),
            "session must be cleared"
        );
    }

    #[tokio::test]
    async fn test_ev_session_end_no_session_returns_not_found() {
        let state = AppState::new();
        let result = EvSessionService::end(&state).await;
        assert!(matches!(result, Err(DomainError::NotFound { .. })));
    }

    #[tokio::test]
    async fn test_ev_session_start_conflict() {
        let state = AppState::new();
        let s1 = make_ev_session();
        let s2 = make_ev_session();
        EvSessionService::start(s1, &state)
            .await
            .expect("first start must succeed");
        let result = EvSessionService::start(s2, &state).await;
        assert!(matches!(result, Err(DomainError::SessionConflict(_))));
    }

    #[tokio::test]
    async fn test_ev_session_start_stores_session() {
        let state = AppState::new();
        let session = make_ev_session();
        let id = session.id;
        EvSessionService::start(session, &state)
            .await
            .expect("start must succeed");
        assert_eq!(state.ev_session().await.map(|s| s.id), Some(id));
    }

    #[tokio::test]
    async fn test_ev_session_end_transitions_linked_request() {
        let state = AppState::new();
        let session = make_ev_session();
        let req = make_active_request(session.id);
        let req_id = req.id;
        state.set_ev_session(Some(session)).await;
        state.upsert_request(req).await;

        EvSessionService::end(&state).await.unwrap();

        let requests = state.active_requests().await;
        let updated = requests.iter().find(|r| r.id == req_id).unwrap();
        assert_eq!(updated.status, UserRequestStatus::Completed);
    }

    #[tokio::test]
    async fn test_ev_session_end_only_completes_active_linked_requests() {
        let state = AppState::new();
        let session = make_ev_session();
        let session_id = session.id;

        // Active + matching session → should become Completed
        let active_linked = make_active_request(session_id);
        let active_linked_id = active_linked.id;

        // Cancelled + matching session → must stay Cancelled
        let mut cancelled_linked = make_active_request(session_id);
        cancelled_linked.status = UserRequestStatus::Cancelled;
        let cancelled_linked_id = cancelled_linked.id;

        // Active + different session → must stay Active
        let other_session_id = Uuid::new_v4();
        let active_other = make_active_request(other_session_id);
        let active_other_id = active_other.id;

        state.set_ev_session(Some(session)).await;
        state.upsert_request(active_linked).await;
        state.upsert_request(cancelled_linked).await;
        state.upsert_request(active_other).await;

        EvSessionService::end(&state).await.unwrap();

        let requests = state.active_requests().await;
        let find = |id: Uuid| requests.iter().find(|r| r.id == id).unwrap().status.clone();

        assert_eq!(find(active_linked_id), UserRequestStatus::Completed);
        assert_eq!(find(cancelled_linked_id), UserRequestStatus::Cancelled);
        assert_eq!(find(active_other_id), UserRequestStatus::Active);
        assert!(state.ev_session().await.is_none());
    }

    #[tokio::test]
    async fn test_heater_clear_removes_target() {
        let state = AppState::new();
        let target = HeaterTarget {
            id: Uuid::new_v4(),
            target_temp_c: 55.0,
            ready_by: Utc::now() + chrono::Duration::hours(2),
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
