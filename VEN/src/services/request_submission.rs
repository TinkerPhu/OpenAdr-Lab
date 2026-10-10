//! Submitting and cancelling a user request: the orchestration around the pure builders in
//! `services::user_request` - install the session, record the request, announce the transition,
//! ask for a replan. `POST`/`DELETE /user-requests` only parse and map the outcome to a status.

use chrono::{DateTime, Utc};
use tokio::sync::watch;
use tracing::{info, warn};
use uuid::Uuid;

use crate::controller::user_request::{ClashingSession, CreateUserRequestParams, RequestError};
use crate::controller::{SimReadPort, SimRosterPort};
use crate::entities::asset::{PlanTrigger, PlanTriggerSignal};
use crate::entities::asset_params::{AssetRequestSlice, RequestKind};
use crate::entities::user_request::UserRequest;
use crate::entities::DomainError;
use crate::services::user_request::UserRequestService;
use crate::state::AppState;

/// Why a submission was refused. The route owns the status codes.
#[derive(Debug, thiserror::Error)]
pub enum SubmitError {
    /// The request does not describe a valid shiftable load.
    #[error("{0}")]
    Invalid(String),
    /// A shiftable load with this asset id is already queued.
    #[error("{0}")]
    Duplicate(String),
    /// The EV or heater request was refused (an EV clash among them).
    #[error("{0}")]
    Request(RequestError),
    #[error("unrecognised asset type for EV/heater path")]
    UnrecognisedAsset,
}

/// Create a user request for a shiftable load, the EV or the heater, install the session it
/// owns, and announce it (one `RequestTransition` event, one replan trigger).
///
/// An EV request that clashes with queued sessions is refused unless it names exactly the
/// sessions to displace (`replace_session_ids`); a clash is never displaced silently.
pub async fn submit(
    body: CreateUserRequestParams,
    now: DateTime<Utc>,
    state: &AppState,
    roster: &dyn SimRosterPort,
    sim_read: &dyn SimReadPort,
    trigger_tx: &watch::Sender<PlanTriggerSignal>,
) -> Result<UserRequest, SubmitError> {
    if UserRequestService::is_shiftable(&body) {
        let (user_req, load) =
            UserRequestService::create_shiftable(body, now).map_err(SubmitError::Invalid)?;
        state
            .add_shiftable_load(load.clone())
            .await
            .map_err(|msg| SubmitError::Duplicate(msg.to_string()))?;
        // shiftable-load-as-asset design.md D1: the asset enters the roster at acceptance
        // (started = false), visible to forecasting and the MILP for its whole life.
        if let Err(msg) = roster.add_shiftable(&load).await {
            // Unreachable while add_shiftable_load refuses a reused asset_id.
            warn!(asset_id = %load.asset_id, error = %msg, "add_asset failed after add_shiftable_load succeeded");
        }
        state.upsert_request(user_req.clone()).await;
        announce_request_transition(state, trigger_tx, &user_req, "None", now).await;
        info!(
            request_id = %user_req.id,
            asset_id = %user_req.asset_id,
            power_kw = load.power_kw,
            duration_min = load.duration_min,
            "user request created (shiftable load)"
        );
        return Ok(user_req);
    }

    let slices = request_slices(state, sim_read).await;
    // The asset says what a request against it becomes; its id decides nothing (R-128).
    let kind = slices
        .iter()
        .find(|s| s.id == body.asset_id)
        .and_then(|s| s.request_kind);
    match kind {
        Some(RequestKind::ChargeSession) => {
            submit_charge_session(body, &slices, now, state, trigger_tx).await
        }
        Some(RequestKind::TemperatureTarget) => {
            let (user_req, target) = UserRequestService::create_heater(body, &slices, now)
                .map_err(SubmitError::Request)?;
            state.set_heater_target(Some(target)).await;
            state.upsert_request(user_req.clone()).await;
            announce_request_transition(state, trigger_tx, &user_req, "None", now).await;
            Ok(user_req)
        }
        None => Err(SubmitError::UnrecognisedAsset),
    }
}

/// The EV path: build the session, queue it (or displace exactly the named ones), record and
/// announce the request.
async fn submit_charge_session(
    body: CreateUserRequestParams,
    slices: &[AssetRequestSlice],
    now: DateTime<Utc>,
    state: &AppState,
    trigger_tx: &watch::Sender<PlanTriggerSignal>,
) -> Result<UserRequest, SubmitError> {
    let replace_ids = body.replace_session_ids.clone();
    let (user_req, session) =
        UserRequestService::create_ev(body, slices, now).map_err(SubmitError::Request)?;
    match &replace_ids {
        Some(ids) => state
            .replace_ev_sessions(ids, session)
            .await
            .map(|_| ())
            .map_err(|refusal| RequestError::EvReplaceRejected {
                rejection: refusal.rejection,
                conflicts: refusal.conflicts.iter().map(ClashingSession::of).collect(),
            }),
        None => state.insert_ev_session(session).await.map_err(|clash| {
            RequestError::EvSessionsConflict {
                conflicts: clash.conflicts.iter().map(ClashingSession::of).collect(),
            }
        }),
    }
    .map_err(SubmitError::Request)?;
    state.upsert_request(user_req.clone()).await;
    announce_request_transition(state, trigger_tx, &user_req, "None", now).await;
    Ok(user_req)
}

/// Cancel a user request (`UserRequestService::cancel`) and announce it.
pub async fn cancel_and_announce(
    id: Uuid,
    now: DateTime<Utc>,
    state: &AppState,
    roster: &dyn SimRosterPort,
    trigger_tx: &watch::Sender<PlanTriggerSignal>,
) -> Result<UserRequest, DomainError> {
    let req = UserRequestService::cancel(id, state, roster).await?;
    announce_request_transition(state, trigger_tx, &req, "Active", now).await;
    info!(request_id = %id, "user request cancelled");
    Ok(req)
}

/// What a request is resolved against: each asset's request slice, with the user's comfort-curve
/// override in place of the built-in default (WP4.2, BL-19).
async fn request_slices(state: &AppState, sim_read: &dyn SimReadPort) -> Vec<AssetRequestSlice> {
    let overrides = state.comfort_overrides_map().await;
    let mut slices = sim_read.request_slices().await;
    for slice in &mut slices {
        slice.comfort_rates = crate::entities::comfort::effective_comfort_rates(
            &overrides,
            &slice.id,
            std::mem::take(&mut slice.comfort_rates),
        );
    }
    slices
}

/// The one place a request's status change is recorded in the controller trace and a replan is
/// asked for.
async fn announce_request_transition(
    state: &AppState,
    trigger_tx: &watch::Sender<PlanTriggerSignal>,
    request: &UserRequest,
    from_status: &str,
    now: DateTime<Utc>,
) {
    state
        .push_controller_event(
            crate::controller::trace::ControllerEvent::RequestTransition {
                ts: now,
                request_id: request.id,
                asset_id: request.asset_id.clone(),
                from_status: from_status.to_string(),
                to_status: format!("{:?}", request.status),
            },
        )
        .await;
    let _ = trigger_tx.send(PlanTriggerSignal::bare(PlanTrigger::UserRequest));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::trace::ControllerEvent;
    use crate::entities::asset_params::{AssetParams, EvParams, HeaterParams};
    use crate::services::user_request::tests::{ev_body, heater_body, shiftable_body};
    use crate::simulator::{SimHandle, SimState};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    struct Fixture {
        state: AppState,
        handle: SimHandle,
        trigger_tx: watch::Sender<PlanTriggerSignal>,
        trigger_rx: watch::Receiver<PlanTriggerSignal>,
        now: DateTime<Utc>,
    }

    /// A real `AppState` and a real simulator roster holding an EV and a heater that declares a
    /// 21 °C default request target.
    fn fixture() -> Fixture {
        let heater = HeaterParams {
            default_target_temp_c: Some(21.0),
            ..HeaterParams::default()
        };
        fixture_with(&[
            AssetParams::Ev(EvParams::default()),
            AssetParams::Heater(heater),
        ])
    }

    /// The same, over a roster of the caller's choosing.
    fn fixture_with(params: &[AssetParams]) -> Fixture {
        let now = Utc::now();
        let sim = Arc::new(Mutex::new(SimState::from_params(params, now)));
        let (trigger_tx, mut trigger_rx) =
            watch::channel(PlanTriggerSignal::bare(PlanTrigger::UserRequest));
        trigger_rx.mark_unchanged();
        Fixture {
            state: AppState::new(),
            handle: SimHandle::new(sim),
            trigger_tx,
            trigger_rx,
            now,
        }
    }

    impl Fixture {
        async fn submit(&self, body: CreateUserRequestParams) -> Result<UserRequest, SubmitError> {
            submit(
                body,
                self.now,
                &self.state,
                &self.handle,
                &self.handle,
                &self.trigger_tx,
            )
            .await
        }

        /// The `(from, to)` of every `RequestTransition` recorded for `id`.
        async fn transitions(&self, id: Uuid) -> Vec<(String, String)> {
            self.state
                .controller_trace()
                .await
                .events()
                .into_iter()
                .filter_map(|e| match e {
                    ControllerEvent::RequestTransition {
                        request_id,
                        from_status,
                        to_status,
                        ..
                    } if request_id == id => Some((from_status, to_status)),
                    _ => None,
                })
                .collect()
        }
    }

    #[tokio::test]
    async fn submit_a_shiftable_load_installs_it_records_and_announces_it() {
        let mut f = fixture();
        let req = f.submit(shiftable_body(60, 4)).await.unwrap();
        let loads = f.state.shiftable_loads().await;
        assert_eq!(loads.len(), 1);
        assert_eq!(loads[0].asset_id, req.asset_id);
        assert!(f
            .state
            .active_requests()
            .await
            .iter()
            .any(|r| r.id == req.id));
        assert_eq!(
            f.transitions(req.id).await,
            [("None".into(), "Active".into())]
        );
        assert!(
            f.trigger_rx.has_changed().unwrap(),
            "a replan was asked for"
        );
        f.trigger_rx.mark_unchanged();
    }

    #[tokio::test]
    async fn submit_the_same_shiftable_load_twice_is_a_duplicate() {
        let f = fixture();
        f.submit(shiftable_body(60, 4)).await.unwrap();
        let again = f.submit(shiftable_body(60, 4)).await;
        assert!(matches!(again, Err(SubmitError::Duplicate(_))), "{again:?}");
    }

    #[tokio::test]
    async fn submit_an_ev_request_queues_its_session() {
        let f = fixture();
        let req = f.submit(ev_body(f.now, Some(0.9))).await.unwrap();
        let sessions = f.state.ev_sessions().await;
        assert!(sessions.iter().any(|s| Some(s.id) == req.session_id));
        assert_eq!(f.transitions(req.id).await.len(), 1);
    }

    #[tokio::test]
    async fn submit_an_ev_request_that_clashes_is_refused_without_an_instruction() {
        let f = fixture();
        f.submit(ev_body(f.now, Some(0.9))).await.unwrap();
        let clash = f.submit(ev_body(f.now, Some(0.9))).await;
        assert!(
            matches!(
                clash,
                Err(SubmitError::Request(
                    RequestError::EvSessionsConflict { .. }
                ))
            ),
            "{clash:?}"
        );
    }

    #[tokio::test]
    async fn submit_a_heater_request_without_a_target_aims_for_the_declared_default() {
        let f = fixture();
        let req = f.submit(heater_body(f.now, None)).await.unwrap();
        let target = f
            .state
            .heater_target()
            .await
            .expect("the heater target is set");
        assert_eq!(target.target_temp_c, 21.0);
        assert_eq!(Some(target.id), req.session_id);
    }

    /// R-128: a request is routed by what the asset declares, so an EV and a heater under
    /// other names are served, and a battery (which takes no user request) is refused, as
    /// before, without any id list deciding it.
    #[tokio::test]
    async fn submit_routes_by_what_the_asset_declares_not_by_its_id() {
        use crate::entities::asset_params::BatteryParams;
        let ev2 = EvParams {
            id: "ev2".into(),
            ..EvParams::default()
        };
        let boiler2 = HeaterParams {
            id: "boiler-2".into(),
            default_target_temp_c: Some(21.0),
            ..HeaterParams::default()
        };
        let f = fixture_with(&[
            AssetParams::Ev(ev2),
            AssetParams::Heater(boiler2),
            AssetParams::Battery(BatteryParams::default()),
        ]);

        let mut ev_req = ev_body(f.now, Some(0.9));
        ev_req.asset_id = "ev2".into();
        let req = f.submit(ev_req).await.expect("the EV path serves ev2");
        assert!(f
            .state
            .ev_sessions()
            .await
            .iter()
            .any(|s| Some(s.id) == req.session_id));

        let mut heater_req = heater_body(f.now, None);
        heater_req.asset_id = "boiler-2".into();
        f.submit(heater_req)
            .await
            .expect("the heater path serves boiler-2");
        assert_eq!(
            f.state.heater_target().await.map(|t| t.target_temp_c),
            Some(21.0)
        );

        let mut battery_req = ev_body(f.now, Some(0.9));
        battery_req.asset_id = crate::ids::ASSET_BATTERY.into();
        let refused = f.submit(battery_req).await;
        assert!(
            matches!(refused, Err(SubmitError::UnrecognisedAsset)),
            "{refused:?}"
        );
    }

    #[tokio::test]
    async fn submit_for_an_asset_no_path_serves_is_refused() {
        let f = fixture();
        let mut body = ev_body(f.now, Some(0.9));
        body.asset_id = "pv".into();
        let refused = f.submit(body).await;
        assert!(
            matches!(refused, Err(SubmitError::UnrecognisedAsset)),
            "{refused:?}"
        );
    }

    #[tokio::test]
    async fn cancel_and_announce_records_the_cancellation_and_asks_for_a_replan() {
        let mut f = fixture();
        let req = f.submit(ev_body(f.now, Some(0.9))).await.unwrap();
        f.trigger_rx.mark_unchanged();
        let cancelled = cancel_and_announce(req.id, f.now, &f.state, &f.handle, &f.trigger_tx)
            .await
            .unwrap();
        assert_eq!(format!("{:?}", cancelled.status), "Cancelled");
        assert_eq!(
            f.transitions(req.id).await.last().cloned(),
            Some(("Active".into(), "Cancelled".into()))
        );
        assert!(f.trigger_rx.has_changed().unwrap());
    }
}
