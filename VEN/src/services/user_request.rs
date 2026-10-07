use chrono::{DateTime, Utc};
use tracing::info;
use uuid::Uuid;

use crate::controller::user_request::{create_from_body, CreateUserRequestParams, RequestError};
use crate::entities::asset_params::AssetRequestSlice;
use crate::entities::device_session::{EvSession, HeaterTarget, ShiftableLoad};
use crate::entities::user_request::{SessionType, UserRequest, UserRequestStatus};
use crate::entities::DomainError;
use crate::ids;
use crate::simulator::SimState;
use crate::state::AppState;

pub struct UserRequestService;

impl UserRequestService {
    /// Create a user request for the EV asset: creates an EvSession linked to the request.
    /// Returns both objects; the caller stores them in state.
    pub fn create_ev(
        body: CreateUserRequestParams,
        asset_data: &[AssetRequestSlice],
        now: DateTime<Utc>,
    ) -> Result<(UserRequest, EvSession), RequestError> {
        let soft_deadline = body.soft_deadline;
        // When the vehicle becomes available for this session. Reuses the request's
        // existing `earliest_start` rather than adding an EV-specific field: "the
        // earliest this may begin" is exactly what a charging window's start is.
        // Absent means available now, which is what the single-slot era implied.
        let stated_window_start = body.earliest_start;
        // The pair is enforced once, here, at the boundary that parses a submission.
        // Below this line the domain type carries a whole estimate or none, so no
        // reader downstream has to remember to check for the half-stated case.
        let (expected_trip_distance_km, expected_return_time) =
            match (body.expected_trip_distance_km, body.expected_return_time) {
                (Some(km), Some(back)) => (Some(km), Some(back)),
                (None, None) => (None, None),
                (km, _back) => {
                    return Err(RequestError::IncompleteTripEstimate {
                        has_distance: km.is_some(),
                    })
                }
            };
        let mut req = create_from_body(body, asset_data, now)?;

        let departure = req
            .deadlines
            .first()
            .map(|d| d.latest_end)
            .unwrap_or_else(|| now + chrono::Duration::hours(8));
        let target_soc = req.target_soc.unwrap_or(0.9);
        let window_start = stated_window_start.unwrap_or(now);
        if window_start >= departure {
            return Err(RequestError::EmptyChargingWindow {
                earliest_start: window_start,
                latest_end: departure,
            });
        }
        let session = EvSession {
            id: Uuid::new_v4(),
            target_soc,
            window_start,
            expected_trip_distance_km,
            expected_return_time,
            departure_time: departure,
            soft_deadline: soft_deadline.unwrap_or(false),
            mode: req.mode.clone(),
            origin: crate::entities::device_session::EvSessionOrigin::UserRequest,
            budget_eur: req.budget_eur,
            comfort_rates: req.comfort_rates.clone(),
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
        body: CreateUserRequestParams,
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
            comfort_rates: req.comfort_rates.clone(),
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
    ///
    /// The single place a shiftable-load request is built and validated:
    /// `POST /user-requests` used to construct the same pair inline, with its
    /// own window-length check this function did not have — two answers to one
    /// question, which is what the `Err` arms below now settle.
    pub fn create_shiftable(
        body: CreateUserRequestParams,
        now: DateTime<Utc>,
    ) -> Result<(UserRequest, ShiftableLoad), String> {
        let power = body
            .power_kw
            .ok_or_else(|| "power_kw required for shiftable load".to_string())?;
        let duration = body
            .duration_min
            .ok_or_else(|| "duration_min required for shiftable load".to_string())?;
        let earliest = body.earliest_start.unwrap_or(now);
        let latest = body
            .latest_end
            .ok_or_else(|| "latest_end required for shiftable load".to_string())?;
        if latest - earliest < chrono::Duration::minutes(duration as i64) {
            return Err(
                "the [earliest_start, latest_end] window is too short for duration_min".to_string(),
            );
        }

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
            comfort_rates: vec![],
            status: UserRequestStatus::Active,
            estimated_cost_eur: 0.0,
            estimated_co2_g: 0.0,
            accumulated_cost_eur: 0.0,
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
    /// - `DomainError::SessionConflict` if the request is already in a terminal
    ///   state, or (shiftable-load-as-asset design.md D6) if it is a shiftable
    ///   load that has already started — physically non-interruptible once
    ///   running, so cancelling it now is refused rather than silently
    ///   desyncing the request from the asset still actually drawing power.
    pub async fn cancel(
        id: Uuid,
        state: &AppState,
        sim: &mut SimState,
    ) -> Result<UserRequest, DomainError> {
        // Check existence and terminal state before calling the state method.
        let requests = state.active_requests().await;
        let req = requests
            .iter()
            .find(|r| r.id == id)
            .ok_or(DomainError::NotFound { id })?
            .clone();

        if matches!(
            req.status,
            UserRequestStatus::Cancelled | UserRequestStatus::Completed
        ) {
            return Err(DomainError::SessionConflict(format!(
                "user request '{id}' is already in a terminal state"
            )));
        }

        if req.session_type == Some(SessionType::ShiftableLoad) {
            let cancellable = sim
                .find_asset(&req.asset_id)
                .is_none_or(|(entry, cfg)| cfg.is_cancellable(&entry.state));
            if !cancellable {
                return Err(DomainError::SessionConflict(format!(
                    "shiftable load '{}' has already started and cannot be cancelled",
                    req.asset_id
                )));
            }
            // Not yet started: drop the pending asset too, not just the
            // HEMS-level request record `cancel_request` clears below.
            sim.remove_asset(&req.asset_id);
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
    pub fn is_shiftable(body: &CreateUserRequestParams) -> bool {
        body.power_kw.is_some() && body.duration_min.is_some()
    }

    pub fn is_ev(body: &CreateUserRequestParams) -> bool {
        body.asset_id == ids::ASSET_EV
    }

    pub fn is_heater(body: &CreateUserRequestParams) -> bool {
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
        let body = CreateUserRequestParams {
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
            expected_trip_distance_km: None,
            soft_deadline: None,
            target_temp_c: None,
            expected_return_time: None,
            replace_session_ids: None,
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

    fn shiftable_body(duration_min: u32, window_h: i64) -> CreateUserRequestParams {
        CreateUserRequestParams {
            mode: Default::default(),
            asset_id: "washing_machine".to_string(),
            power_kw: Some(2.0),
            duration_min: Some(duration_min),
            earliest_start: Some(Utc::now()),
            expected_trip_distance_km: None,
            latest_end: Some(Utc::now() + chrono::Duration::hours(window_h)),
            target_soc: None,
            target_energy_kwh: None,
            desired_power_kw: None,
            deadlines: vec![],
            completion_policy: None,
            comfort_rates: None,
            budget_eur: None,
            interruptible: None,
            tolerance_min: None,
            soft_deadline: None,
            target_temp_c: None,
            expected_return_time: None,
            replace_session_ids: None,
        }
    }

    /// A 2 h run inside a 1 h window cannot be placed. The route used to check
    /// this on its own while this function did not, so whichever caller came
    /// second got a different answer to the same question.
    #[test]
    fn create_shiftable_rejects_a_window_shorter_than_the_run() {
        let err = UserRequestService::create_shiftable(shiftable_body(120, 1), Utc::now())
            .expect_err("a 2 h run does not fit a 1 h window");
        assert!(
            err.contains("too short"),
            "error must name the window, got: {err}"
        );
    }

    #[test]
    fn create_shiftable_accepts_a_window_exactly_as_long_as_the_run() {
        assert!(UserRequestService::create_shiftable(shiftable_body(60, 1), Utc::now()).is_ok());
    }

    /// `latest_end` is what bounds the placement; without it there is no
    /// window at all.
    #[test]
    fn create_shiftable_rejects_a_missing_latest_end() {
        let mut body = shiftable_body(60, 1);
        body.latest_end = None;
        let err = UserRequestService::create_shiftable(body, Utc::now())
            .expect_err("latest_end is required");
        assert!(err.contains("latest_end"), "got: {err}");
    }

    /// `create_shiftable` used to `unwrap()` these, which is only safe behind
    /// an `is_shiftable` check the caller had to remember.
    #[test]
    fn create_shiftable_rejects_a_body_that_is_not_a_shiftable_load() {
        let mut body = shiftable_body(60, 1);
        body.power_kw = None;
        assert!(UserRequestService::create_shiftable(body, Utc::now()).is_err());
    }

    /// Cancelling an unknown id returns CancelError::NotFound.
    #[tokio::test]
    async fn test_cancel_unknown_id_returns_err() {
        let state = AppState::new();
        let unknown = Uuid::new_v4();
        let mut sim = SimState::from_params(&[], Utc::now());
        let result = UserRequestService::cancel(unknown, &state, &mut sim).await;
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
            comfort_rates: vec![],
            estimated_cost_eur: 0.0,
            estimated_co2_g: 0.0,
            accumulated_cost_eur: 0.0,
            interruptible: false,
            tolerance_min: None,
            budget_eur: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let id = req.id;
        state.upsert_request(req).await;

        let mut sim = SimState::from_params(&[], Utc::now());
        let result = UserRequestService::cancel(id, &state, &mut sim).await;
        assert!(matches!(result, Err(DomainError::SessionConflict(_))));
    }

    /// Cancelling an active EV request sets status to Cancelled.
    #[tokio::test]
    async fn test_cancel_sets_cancelled_and_clears_ev_session() {
        let state = AppState::new();
        let ev_session = EvSession {
            mode: Default::default(),
            origin: crate::entities::device_session::EvSessionOrigin::UserRequest,
            id: Uuid::new_v4(),
            target_soc: 0.8,
            window_start: Utc::now(),
            expected_trip_distance_km: None,
            expected_return_time: None,
            departure_time: Utc::now() + chrono::Duration::hours(6),
            soft_deadline: false,
            budget_eur: None,
            comfort_rates: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let session_id = ev_session.id;
        state.insert_ev_session(ev_session).await.unwrap();

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
            comfort_rates: vec![],
            estimated_cost_eur: 0.0,
            estimated_co2_g: 0.0,
            accumulated_cost_eur: 0.0,
            interruptible: false,
            tolerance_min: None,
            budget_eur: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let id = req.id;
        state.upsert_request(req).await;

        let mut sim = SimState::from_params(&[], Utc::now());
        let cancelled = UserRequestService::cancel(id, &state, &mut sim)
            .await
            .unwrap();
        assert_eq!(cancelled.status, UserRequestStatus::Cancelled);
        // The cancelled request's own session must be gone from the queue.
        assert!(state.ev_sessions().await.is_empty());
    }

    /// A shiftable-load request as `create_shiftable` builds it, for the asset `asset_id`.
    fn shiftable_request(asset_id: &str) -> UserRequest {
    UserRequest {
        mode: Default::default(),
        id: Uuid::new_v4(),
        asset_id: asset_id.to_string(),
        status: UserRequestStatus::Active,
        target_soc: None,
        target_energy_kwh: 2.0,
        desired_power_kw: 2.0,
        deadlines: vec![],
        completion_policy: "STOP".to_string(),
        max_total_cost_eur: None,
        tier_count: 0,
        session_id: Some(Uuid::new_v4()),
        session_type: Some(SessionType::ShiftableLoad),
        comfort_rates: vec![],
        estimated_cost_eur: 0.0,
        estimated_co2_g: 0.0,
        accumulated_cost_eur: 0.0,
        interruptible: false,
        tolerance_min: None,
        budget_eur: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        }
    }

    /// shiftable-load-as-asset design.md D6: a shiftable load that has
    /// already started cannot be cancelled — the physics is non-interruptible,
    /// so silently removing the asset would desync accounting from the power
    /// it's still actually drawing.
    #[tokio::test]
    async fn test_cancel_rejects_a_started_shiftable_load() {
        let state = AppState::new();
        let mut sim = SimState::from_params(&[], Utc::now());
        sim.add_shiftable(
            "wm",
            crate::assets::ShiftableLoadAsset {
                power_kw: 2.0,
                duration_min: 60,
                earliest_start: Utc::now(),
                latest_end: Utc::now() + chrono::Duration::hours(4),
            },
        )
        .unwrap();
        sim.asset_mut("wm").unwrap().state =
            crate::assets::AssetState::ShiftableLoad(crate::assets::ShiftableLoadState {
                started: true,
                elapsed_min: 5.0,
                actual_power_kw: 2.0,
            });

        let req = shiftable_request("wm");
        let id = req.id;
        state.upsert_request(req).await;

        let result = UserRequestService::cancel(id, &state, &mut sim).await;
        assert!(
            matches!(result, Err(DomainError::SessionConflict(_))),
            "cancelling a started shiftable load must be rejected"
        );
        assert!(
            sim.find_asset("wm").is_some(),
            "the running asset must not be removed"
        );
    }

    /// What the sim actually does: a load commanded on is drawing power and has started, so
    /// cancelling it is refused (the E2E scenario hit a 204 here).
    #[tokio::test]
    async fn cancel_rejects_a_shiftable_load_the_sim_has_started_running() {
        let state = AppState::new();
        let mut sim = SimState::from_params(&[], Utc::now());
        sim.add_shiftable(
            "wm",
            crate::assets::ShiftableLoadAsset {
                power_kw: 2.0,
                duration_min: 60,
                earliest_start: Utc::now(),
                latest_end: Utc::now() + chrono::Duration::hours(4),
            },
        )
        .unwrap();
        sim.tick(crate::simulator::TickInputs::new(
            1.0,
            Utc::now(),
            std::collections::HashMap::from([("wm".to_string(), 2.0)]),
        ));
        assert!(sim.asset("wm").unwrap().last_power_kw > 0.0, "the load is drawing power");

        let req = shiftable_request("wm");
        let id = req.id;
        state.upsert_request(req).await;

        let result = UserRequestService::cancel(id, &state, &mut sim).await;
        assert!(
            matches!(result, Err(DomainError::SessionConflict(_))),
            "a running load cannot be cancelled, got {result:?}"
        );
    }

    /// A not-yet-started shiftable load can be cancelled; its pending asset
    /// is removed from `SimState` along with the HEMS-level request.
    #[tokio::test]
    async fn test_cancel_removes_a_pending_shiftable_loads_asset() {
        let state = AppState::new();
        let mut sim = SimState::from_params(&[], Utc::now());
        sim.add_shiftable(
            "wm",
            crate::assets::ShiftableLoadAsset {
                power_kw: 2.0,
                duration_min: 60,
                earliest_start: Utc::now(),
                latest_end: Utc::now() + chrono::Duration::hours(4),
            },
        )
        .unwrap();

        let req = shiftable_request("wm");
        let id = req.id;
        state.upsert_request(req).await;

        let cancelled = UserRequestService::cancel(id, &state, &mut sim)
            .await
            .unwrap();
        assert_eq!(cancelled.status, UserRequestStatus::Cancelled);
        assert!(
            sim.find_asset("wm").is_none(),
            "the pending asset must be removed"
        );
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
        let body = CreateUserRequestParams {
            mode: Default::default(),
            asset_id: ids::ASSET_EV.to_string(),
            target_soc: Some(0.9),
            target_energy_kwh: None,
            desired_power_kw: None,
            deadlines: vec![crate::controller::user_request::RequestDeadlineParams {
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
            expected_trip_distance_km: None,
            latest_end: None,
            soft_deadline: None,
            target_temp_c: None,
            expected_return_time: None,
            replace_session_ids: None,
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
        let body = CreateUserRequestParams {
            asset_id: ids::ASSET_EV.to_string(),
            target_soc: Some(0.9),
            target_energy_kwh: None,
            desired_power_kw: None,
            deadlines: vec![crate::controller::user_request::RequestDeadlineParams {
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
            expected_trip_distance_km: None,
            latest_end: None,
            soft_deadline: None,
            target_temp_c: None,
            mode: Some(UserRequestMode::Asap),
            expected_return_time: None,
            replace_session_ids: None,
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
        let body = CreateUserRequestParams {
            asset_id: ids::ASSET_HEATER.to_string(),
            target_soc: None,
            target_energy_kwh: Some(5.0),
            desired_power_kw: Some(2.0),
            deadlines: vec![crate::controller::user_request::RequestDeadlineParams {
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
            expected_trip_distance_km: None,
            latest_end: None,
            soft_deadline: None,
            target_temp_c: Some(55.0),
            mode: None,
            expected_return_time: None,
            replace_session_ids: None,
        };
        let (req, target) =
            UserRequestService::create_heater(body, &[heater_slice()], now).unwrap();
        assert_eq!(req.mode, UserRequestMode::ByDeadline);
        assert_eq!(target.mode, UserRequestMode::ByDeadline);
    }

    #[test]
    fn test_create_ev_unknown_asset_returns_err() {
        let now = Utc::now();
        let body = CreateUserRequestParams {
            mode: Default::default(),
            asset_id: "nonexistent".to_string(),
            target_soc: Some(0.9),
            target_energy_kwh: None,
            desired_power_kw: None,
            deadlines: vec![crate::controller::user_request::RequestDeadlineParams {
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
            expected_trip_distance_km: None,
            latest_end: None,
            soft_deadline: None,
            target_temp_c: None,
            expected_return_time: None,
            replace_session_ids: None,
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
        let body = CreateUserRequestParams {
            mode: Default::default(),
            asset_id: ids::ASSET_HEATER.to_string(),
            target_soc: None,
            target_energy_kwh: Some(5.0),
            desired_power_kw: Some(2.0),
            deadlines: vec![crate::controller::user_request::RequestDeadlineParams {
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
            expected_trip_distance_km: None,
            latest_end: None,
            soft_deadline: None,
            target_temp_c: Some(55.0),
            expected_return_time: None,
            replace_session_ids: None,
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
        let base = CreateUserRequestParams {
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
            expected_trip_distance_km: None,
            latest_end: None,
            soft_deadline: None,
            target_temp_c: None,
            expected_return_time: None,
            replace_session_ids: None,
        };
        assert!(UserRequestService::is_shiftable(&base));

        let ev_body = CreateUserRequestParams {
            asset_id: ids::ASSET_EV.to_string(),
            power_kw: None,
            duration_min: None,
            expected_return_time: None,
            replace_session_ids: None,
            ..base
        };
        assert!(UserRequestService::is_ev(&ev_body));
        assert!(!UserRequestService::is_shiftable(&ev_body));
    }
}
