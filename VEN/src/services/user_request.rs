use chrono::{DateTime, Utc};
use tracing::info;
use uuid::Uuid;

use crate::controller::user_request::{create_from_body, CreateUserRequestBody, RequestError};
use crate::entities::asset_params::AssetRequestSlice;
use crate::entities::device_session::{EvSession, HeaterTarget, ShiftableLoad};
use crate::entities::user_request::{UserRequest, UserRequestStatus};
use crate::entities::DomainError;
use crate::ids;
use crate::state::AppState;

pub struct UserRequestService;

impl UserRequestService {
    /// Create a user request for the EV asset: creates an EvSession linked to the request.
    /// Returns both objects; the caller stores them in state.
    pub fn create_ev(
        body: CreateUserRequestBody,
        asset_data: &[AssetRequestSlice],
        now: DateTime<Utc>,
    ) -> Result<(UserRequest, EvSession), RequestError> {
        let soft_deadline = body.soft_deadline;
        let mut req = create_from_body(body, asset_data, now)?;

        let departure = req
            .deadlines
            .first()
            .map(|d| d.latest_end)
            .unwrap_or_else(|| now + chrono::Duration::hours(8));
        let target_soc = req.target_soc.unwrap_or(0.9);
        let session = EvSession {
            id: Uuid::new_v4(),
            target_soc,
            departure_time: departure,
            soft_deadline: soft_deadline.unwrap_or(false),
            mode: req.mode.clone(),
            created_at: now,
            updated_at: now,
        };

        req.session_id = Some(session.id);
        req.session_type = Some(crate::entities::user_request::SessionType::Ev);

        info!(
            request_id = %req.id,
            session_id = %session.id,
            asset_id = %req.asset_id,
            target_soc,
            "user request created (EV session)"
        );
        Ok((req, session))
    }

    /// Create a user request for the heater/boiler asset: creates a HeaterTarget linked to the request.
    pub fn create_heater(
        body: CreateUserRequestBody,
        asset_data: &[AssetRequestSlice],
        now: DateTime<Utc>,
    ) -> Result<(UserRequest, HeaterTarget), RequestError> {
        let target_temp_c = body.target_temp_c;
        let mut req = create_from_body(body, asset_data, now)?;

        let ready_by = req
            .deadlines
            .first()
            .map(|d| d.latest_end)
            .unwrap_or_else(|| now + chrono::Duration::hours(4));
        let target_temp_c = target_temp_c.unwrap_or(55.0);
        let target = HeaterTarget {
            id: Uuid::new_v4(),
            target_temp_c,
            ready_by,
            mode: req.mode.clone(),
            created_at: now,
            updated_at: now,
        };

        req.session_id = Some(target.id);
        req.session_type = Some(crate::entities::user_request::SessionType::Heater);

        info!(
            request_id = %req.id,
            session_id = %target.id,
            asset_id = %req.asset_id,
            target_temp_c,
            "user request created (heater target)"
        );
        Ok((req, target))
    }

    /// Create a user request for a shiftable load. No sim-asset lookup required.
    // Not yet wired to a route — shiftable loads are created inline in routes/hems.rs.
    #[allow(dead_code)]
    pub fn create_shiftable(
        body: CreateUserRequestBody,
        now: DateTime<Utc>,
    ) -> Result<(UserRequest, ShiftableLoad), String> {
        let power = body.power_kw.unwrap();
        let duration = body.duration_min.unwrap();
        let earliest = body.earliest_start.unwrap_or(now);
        let latest = body
            .latest_end
            .ok_or_else(|| "latest_end required for shiftable load".to_string())?;

        let mode = body.mode.clone().unwrap_or_default();
        let load = ShiftableLoad {
            id: Uuid::new_v4(),
            asset_id: body.asset_id.clone(),
            power_kw: power,
            duration_min: duration,
            earliest_start: earliest,
            latest_end: latest,
            mode: mode.clone(),
            created_at: now,
            updated_at: now,
        };

        let user_req = UserRequest {
            id: Uuid::new_v4(),
            asset_id: body.asset_id,
            target_soc: None,
            target_energy_kwh: (power * duration as f64) / 60.0,
            desired_power_kw: power,
            deadlines: vec![],
            mode,
            completion_policy: "STOP".to_string(),
            max_total_cost_eur: None,
            tier_count: 0,
            session_id: Some(load.id),
            session_type: Some(crate::entities::user_request::SessionType::ShiftableLoad),
            status: UserRequestStatus::Active,
            estimated_cost_eur: 0.0,
            estimated_co2_g: 0.0,
            interruptible: body.interruptible.unwrap_or(false),
            tolerance_min: body.tolerance_min,
            budget_eur: body.budget_eur,
            created_at: now,
            updated_at: now,
        };

        Ok((user_req, load))
    }

    /// Cancel a user request by id.
    ///
    /// Returns the cancelled request on success, or:
    /// - `DomainError::NotFound` if no request with that id exists.
    /// - `DomainError::SessionConflict` if the request is already in a terminal state.
    pub async fn cancel(id: Uuid, state: &AppState) -> Result<UserRequest, DomainError> {
        // Check existence and terminal state before calling the state method.
        let requests = state.active_requests().await;
        let req = requests
            .iter()
            .find(|r| r.id == id)
            .ok_or(DomainError::NotFound { id })?;

        if matches!(
            req.status,
            UserRequestStatus::Cancelled | UserRequestStatus::Completed
        ) {
            return Err(DomainError::SessionConflict(format!(
                "user request '{id}' is already in a terminal state"
            )));
        }

        // Delegate to AppState which handles session clearing atomically.
        state.cancel_request(id).await;

        // Return the now-cancelled request.
        let updated = state
            .active_requests()
            .await
            .into_iter()
            .find(|r| r.id == id)
            .ok_or(DomainError::NotFound { id })?;

        Ok(updated)
    }

    /// Determine which creation path to use based on the request body.
    // Not yet wired to a route — shiftable detection is done inline in routes/hems.rs.
    #[allow(dead_code)]
    pub fn is_shiftable(body: &CreateUserRequestBody) -> bool {
        body.power_kw.is_some() && body.duration_min.is_some()
    }

    pub fn is_ev(body: &CreateUserRequestBody) -> bool {
        body.asset_id == ids::ASSET_EV
    }

    pub fn is_heater(body: &CreateUserRequestBody) -> bool {
        body.asset_id == ids::ASSET_HEATER || body.asset_id == ids::ASSET_BOILER
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::user_request::UserRequestStatus;
    use crate::entities::DomainError;

    /// Check shiftable request creation from a minimal body.
    #[test]
    fn test_create_shiftable_builds_load() {
        let body = CreateUserRequestBody {
            mode: Default::default(),
            asset_id: "washing_machine".to_string(),
            power_kw: Some(2.0),
            duration_min: Some(60),
            latest_end: Some(Utc::now() + chrono::Duration::hours(4)),
            target_soc: None,
            target_energy_kwh: None,
            desired_power_kw: None,
            deadlines: vec![],
            completion_policy: None,
            comfort_rates: None,
            budget_eur: None,
            interruptible: None,
            tolerance_min: None,
            earliest_start: None,
            soft_deadline: None,
            target_temp_c: None,
        };
        let now = Utc::now();
        let (req, load) = UserRequestService::create_shiftable(body, now).unwrap();

        assert_eq!(req.asset_id, "washing_machine");
        assert_eq!(req.status, UserRequestStatus::Active);
        assert_eq!(req.session_id, Some(load.id));
        assert!((req.target_energy_kwh - 2.0).abs() < 0.001); // 2kW * 60min / 60 = 2kWh
        assert_eq!(load.power_kw, 2.0);
        assert_eq!(load.duration_min, 60);
    }

    /// Cancelling an unknown id returns CancelError::NotFound.
    #[tokio::test]
    async fn test_cancel_unknown_id_returns_err() {
        let state = AppState::new();
        let unknown = Uuid::new_v4();
        let result = UserRequestService::cancel(unknown, &state).await;
        assert!(matches!(result, Err(DomainError::NotFound { .. })));
    }

    /// Cancelling a request that is already Cancelled returns AlreadyTerminal.
    #[tokio::test]
    async fn test_cancel_terminal_request_returns_err() {
        let state = AppState::new();
        // Insert a pre-cancelled request.
        let req = UserRequest {
            mode: Default::default(),
            id: Uuid::new_v4(),
            asset_id: "ev".to_string(),
            status: UserRequestStatus::Cancelled,
            target_soc: None,
            target_energy_kwh: 10.0,
            desired_power_kw: 3.0,
            deadlines: vec![],
            completion_policy: "STOP".to_string(),
            max_total_cost_eur: None,
            tier_count: 0,
            session_id: None,
            session_type: None,
            estimated_cost_eur: 0.0,
            estimated_co2_g: 0.0,
            interruptible: false,
            tolerance_min: None,
            budget_eur: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let id = req.id;
        state.upsert_request(req).await;

        let result = UserRequestService::cancel(id, &state).await;
        assert!(matches!(result, Err(DomainError::SessionConflict(_))));
    }

    /// Cancelling an active EV request sets status to Cancelled.
    #[tokio::test]
    async fn test_cancel_sets_cancelled_and_clears_ev_session() {
        let state = AppState::new();
        let ev_session = EvSession {
            mode: Default::default(),
            id: Uuid::new_v4(),
            target_soc: 0.8,
            departure_time: Utc::now() + chrono::Duration::hours(6),
            soft_deadline: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let session_id = ev_session.id;
        state.set_ev_session(Some(ev_session)).await;

        let req = UserRequest {
            mode: Default::default(),
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
        };
        let id = req.id;
        state.upsert_request(req).await;

        let cancelled = UserRequestService::cancel(id, &state).await.unwrap();
        assert_eq!(cancelled.status, UserRequestStatus::Cancelled);
        // EV session must be cleared.
        assert!(state.ev_session().await.is_none());
    }

    fn ev_slice(soc: f64) -> AssetRequestSlice {
        use crate::entities::asset::{ComfortRate, CompletionPolicy};
        AssetRequestSlice {
            id: ids::ASSET_EV.to_string(),
            current_soc: Some(soc),
            default_soc_target: Some(0.8),
            capacity_kwh: Some(60.0),
            max_charge_kw: Some(7.4),
            completion_policy: CompletionPolicy::Stop,
            comfort_rates: vec![ComfortRate {
                fill: 0.8,
                max_marginal_price: 0.3,
                max_marginal_co2: 0.0,
            }],
        }
    }

    fn heater_slice() -> AssetRequestSlice {
        use crate::entities::asset::{ComfortRate, CompletionPolicy};
        AssetRequestSlice {
            id: ids::ASSET_HEATER.to_string(),
            current_soc: None,
            default_soc_target: None,
            capacity_kwh: None,
            max_charge_kw: None,
            completion_policy: CompletionPolicy::Stop,
            comfort_rates: vec![ComfortRate {
                fill: 0.0,
                max_marginal_price: 0.0,
                max_marginal_co2: 0.0,
            }],
        }
    }

    #[test]
    fn test_create_ev_builds_session() {
        let now = Utc::now();
        let body = CreateUserRequestBody {
            mode: Default::default(),
            asset_id: ids::ASSET_EV.to_string(),
            target_soc: Some(0.9),
            target_energy_kwh: None,
            desired_power_kw: None,
            deadlines: vec![crate::controller::user_request::RequestDeadlineInput {
                latest_end: now + chrono::Duration::hours(6),
                max_total_cost_eur: None,
                max_marginal_rate_eur_kwh: None,
                min_completion: None,
            }],
            completion_policy: None,
            comfort_rates: None,
            budget_eur: None,
            interruptible: None,
            tolerance_min: None,
            power_kw: None,
            duration_min: None,
            earliest_start: None,
            latest_end: None,
            soft_deadline: None,
            target_temp_c: None,
        };
        let (req, session) = UserRequestService::create_ev(body, &[ev_slice(0.5)], now).unwrap();
        assert_eq!(req.asset_id, ids::ASSET_EV);
        // soc 0.5 → target 0.9: (0.9-0.5)*60 = 24 kWh
        assert!((req.target_energy_kwh - 24.0).abs() < 0.01);
        assert_eq!(req.session_id, Some(session.id));
        assert!((session.target_soc - 0.9).abs() < 0.01);
    }

    /// A mode given in the body must land on both the UserRequest and the EvSession.
    #[test]
    fn test_create_ev_mode_passthrough_to_session() {
        use crate::entities::design_vocabulary::UserRequestMode;
        let now = Utc::now();
        let body = CreateUserRequestBody {
            asset_id: ids::ASSET_EV.to_string(),
            target_soc: Some(0.9),
            target_energy_kwh: None,
            desired_power_kw: None,
            deadlines: vec![crate::controller::user_request::RequestDeadlineInput {
                latest_end: now + chrono::Duration::hours(6),
                max_total_cost_eur: None,
                max_marginal_rate_eur_kwh: None,
                min_completion: None,
            }],
            completion_policy: None,
            comfort_rates: None,
            budget_eur: None,
            interruptible: None,
            tolerance_min: None,
            power_kw: None,
            duration_min: None,
            earliest_start: None,
            latest_end: None,
            soft_deadline: None,
            target_temp_c: None,
            mode: Some(UserRequestMode::Asap),
        };
        let (req, session) = UserRequestService::create_ev(body, &[ev_slice(0.5)], now).unwrap();
        assert_eq!(req.mode, UserRequestMode::Asap);
        assert_eq!(session.mode, UserRequestMode::Asap);
    }

    /// Omitting the mode falls back to BY_DEADLINE (today's implicit behaviour).
    #[test]
    fn test_create_heater_missing_mode_defaults_by_deadline() {
        use crate::entities::design_vocabulary::UserRequestMode;
        let now = Utc::now();
        let body = CreateUserRequestBody {
            asset_id: ids::ASSET_HEATER.to_string(),
            target_soc: None,
            target_energy_kwh: Some(5.0),
            desired_power_kw: Some(2.0),
            deadlines: vec![crate::controller::user_request::RequestDeadlineInput {
                latest_end: now + chrono::Duration::hours(4),
                max_total_cost_eur: None,
                max_marginal_rate_eur_kwh: None,
                min_completion: None,
            }],
            completion_policy: None,
            comfort_rates: None,
            budget_eur: None,
            interruptible: None,
            tolerance_min: None,
            power_kw: None,
            duration_min: None,
            earliest_start: None,
            latest_end: None,
            soft_deadline: None,
            target_temp_c: Some(55.0),
            mode: None,
        };
        let (req, target) =
            UserRequestService::create_heater(body, &[heater_slice()], now).unwrap();
        assert_eq!(req.mode, UserRequestMode::ByDeadline);
        assert_eq!(target.mode, UserRequestMode::ByDeadline);
    }

    #[test]
    fn test_create_ev_unknown_asset_returns_err() {
        let now = Utc::now();
        let body = CreateUserRequestBody {
            mode: Default::default(),
            asset_id: "nonexistent".to_string(),
            target_soc: Some(0.9),
            target_energy_kwh: None,
            desired_power_kw: None,
            deadlines: vec![crate::controller::user_request::RequestDeadlineInput {
                latest_end: now + chrono::Duration::hours(6),
                max_total_cost_eur: None,
                max_marginal_rate_eur_kwh: None,
                min_completion: None,
            }],
            completion_policy: None,
            comfort_rates: None,
            budget_eur: None,
            interruptible: None,
            tolerance_min: None,
            power_kw: None,
            duration_min: None,
            earliest_start: None,
            latest_end: None,
            soft_deadline: None,
            target_temp_c: None,
        };
        let result = UserRequestService::create_ev(body, &[ev_slice(0.5)], now);
        assert!(matches!(
            result,
            Err(crate::controller::user_request::RequestError::UnknownAsset(
                _
            ))
        ));
    }

    #[test]
    fn test_create_heater_builds_target() {
        let now = Utc::now();
        let body = CreateUserRequestBody {
            mode: Default::default(),
            asset_id: ids::ASSET_HEATER.to_string(),
            target_soc: None,
            target_energy_kwh: Some(5.0),
            desired_power_kw: Some(2.0),
            deadlines: vec![crate::controller::user_request::RequestDeadlineInput {
                latest_end: now + chrono::Duration::hours(4),
                max_total_cost_eur: None,
                max_marginal_rate_eur_kwh: None,
                min_completion: None,
            }],
            completion_policy: None,
            comfort_rates: None,
            budget_eur: None,
            interruptible: None,
            tolerance_min: None,
            power_kw: None,
            duration_min: None,
            earliest_start: None,
            latest_end: None,
            soft_deadline: None,
            target_temp_c: Some(55.0),
        };
        let (req, target) =
            UserRequestService::create_heater(body, &[heater_slice()], now).unwrap();
        assert_eq!(req.asset_id, ids::ASSET_HEATER);
        assert!((req.target_energy_kwh - 5.0).abs() < 0.01);
        assert_eq!(req.session_id, Some(target.id));
        assert!((target.target_temp_c - 55.0).abs() < 0.01);
    }

    /// Discriminator helpers correctly categorise request bodies.
    #[test]
    fn test_discriminators() {
        let base = CreateUserRequestBody {
            mode: Default::default(),
            asset_id: String::new(),
            target_soc: None,
            target_energy_kwh: None,
            desired_power_kw: None,
            deadlines: vec![],
            completion_policy: None,
            comfort_rates: None,
            budget_eur: None,
            interruptible: None,
            tolerance_min: None,
            power_kw: Some(1.0),
            duration_min: Some(30),
            earliest_start: None,
            latest_end: None,
            soft_deadline: None,
            target_temp_c: None,
        };
        assert!(UserRequestService::is_shiftable(&base));

        let ev_body = CreateUserRequestBody {
            asset_id: ids::ASSET_EV.to_string(),
            power_kw: None,
            duration_min: None,
            ..base
        };
        assert!(UserRequestService::is_ev(&ev_body));
        assert!(!UserRequestService::is_shiftable(&ev_body));
    }
}
