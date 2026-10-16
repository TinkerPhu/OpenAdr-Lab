use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    Json,
};
use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

use crate::app_ctx::{ActiveObjective, History, PlanTriggerTx};
use crate::entities::history::LedgerPeriod;
use crate::entities::reservation_request::ReservationRequest;
use crate::entities::PlannerObjective;
use crate::planner_events::PlannerEventTx;
use crate::state::AppState;
use lab_core::time_window::TimeWindow;

/// GET /plan — returns the active Plan (null until Stage 3).
pub async fn get_plan(State(state): State<AppState>) -> impl IntoResponse {
    match state.active_plan().await {
        Some(plan) => Json(plan).into_response(),
        None => Json(serde_json::Value::Null).into_response(),
    }
}

/// GET /forecast — per-asset forecasts from the latest plan cycle
/// (WP3.6, BL-15). Empty array until the first plan has been adopted.
pub async fn get_forecast(State(state): State<AppState>) -> impl IntoResponse {
    Json(state.asset_forecasts().await)
}

/// PUT /plan/objective — change the active optimization objective and trigger an immediate replan.
#[derive(Debug, Deserialize)]
pub struct SetObjectiveBody {
    pub objective: PlannerObjective,
}

pub async fn put_plan_objective(
    State(active_objective): State<ActiveObjective>,
    State(trigger_tx): State<PlanTriggerTx>,
    Json(body): Json<SetObjectiveBody>,
) -> impl IntoResponse {
    crate::services::objective::set_objective(
        &active_objective,
        &trigger_tx,
        body.objective,
        crate::entities::objective_setting::ObjectiveSource::VenUi,
        chrono::Utc::now(),
    )
    .await;
    StatusCode::NO_CONTENT
}

/// `/plan/objective`: PUT changes the objective, GET reads the setting.
pub fn plan_objective_methods() -> axum::routing::MethodRouter<crate::app_ctx::AppCtx> {
    axum::routing::put(put_plan_objective).get(get_plan_objective)
}

/// GET /plan/objective — the objective in force, who set it and when, the profile's objective
/// and the objectives this VEN accepts (the Planner page and the Dashboard read it).
pub async fn get_plan_objective(
    State(active_objective): State<ActiveObjective>,
) -> impl IntoResponse {
    Json(active_objective.read().await.clone())
}

/// GET /tariffs — returns planned tariff snapshots parsed from active events.
pub async fn get_tariffs(State(state): State<AppState>) -> impl IntoResponse {
    Json(state.planned_tariffs().await)
}

/// GET /capacity — the capacity state in force now, read from the schedule (GB-48).
pub async fn get_capacity(State(state): State<AppState>) -> impl IntoResponse {
    Json(state.capacity_state_at(chrono::Utc::now()).await)
}

/// GET /capacity/schedule — returns the capacity schedule (per-interval import/export
/// limits, subscriptions and reservations parsed from active events), the timeline
/// `GET /capacity` reads at the current moment.
pub async fn get_capacity_schedule(State(state): State<AppState>) -> impl IntoResponse {
    Json(state.planned_capacity_limits().await)
}

/// `GET /signals`: every live grid signal in one round trip (WP4.6).
#[derive(Debug, Serialize, ts_rs::TS)]
pub struct SignalsState {
    pub alerts: Vec<crate::entities::capacity::AlertWindow>,
    pub simple: Vec<crate::entities::capacity::SimpleWindow>,
    pub dispatch: Vec<crate::entities::capacity::DispatchWindow>,
    pub capacity: crate::entities::capacity::OadrCapacityState,
    pub reservation_request: Option<ReservationRequest>,
}

/// GET /signals — WP4.6: one-round-trip aggregate of the active grid signals
/// (alert / SIMPLE / dispatch windows + capacity state) for the UI status
/// strip. Read-only view over state the poll loop already maintains.
pub async fn get_signals(State(state): State<AppState>) -> impl IntoResponse {
    // Ended windows stay in state while their event exists on the VTN
    // (events are permanent records; operators may end an emergency by
    // adding timing rather than deleting) — drop them here so the strip
    // never shows a stale chip. Upcoming windows are kept (the UI labels
    // them "from HH:MM").
    let now = chrono::Utc::now();
    let alerts: Vec<_> = state
        .alert_windows()
        .await
        .into_iter()
        .filter(|w| !w.is_ended(now))
        .collect();
    let simple: Vec<_> = state
        .simple_windows()
        .await
        .into_iter()
        .filter(|w| !w.is_ended(now))
        .collect();
    let dispatch: Vec<_> = state
        .dispatch_windows()
        .await
        .into_iter()
        .filter(|w| !w.is_ended(now))
        .collect();
    // R-76: what the VEN is asking the VTN for, alongside what the VTN has
    // granted it. `ui-transparency` — this goes out on the wire as
    // `*_RESERVATION_CAPACITY`, so it must be readable here too; the strip
    // shows it next to the subscription and reservation it is measured against.
    let capacity = state.capacity_state_at(now).await;
    let capacity_schedule = state.planned_capacity_limits().await;
    let reservation_request = state
        .site_headroom()
        .await
        .map(|env| ReservationRequest::from_headroom(&env, &capacity_schedule, now));
    Json(SignalsState {
        alerts,
        simple,
        dispatch,
        capacity,
        reservation_request,
    })
}

/// GET /obligations — returns pending report obligations (Stage 2).
pub async fn get_obligations(State(state): State<AppState>) -> impl IntoResponse {
    Json(state.report_obligations().await)
}

#[derive(Debug, Deserialize)]
pub struct LedgerQuery {
    pub asset_id: Option<String>,
}

/// `GET /ledger?asset_id=`: one asset's open period and its archived monthly ones.
#[derive(Debug, Serialize, ts_rs::TS)]
pub struct AssetLedgerHistory {
    pub current: Option<crate::entities::asset_ledger::AssetLedgerEntry>,
    /// Oldest first. `None` when the VEN runs without a history store: the closed periods are
    /// then unknown, which is not the same as there being none.
    pub closed_periods: Option<Vec<LedgerPeriod>>,
}

/// GET /ledger — returns per-asset cumulative energy/cost/CO₂ for the current
/// open billing period (Stage 4), keyed by asset id. With `?asset_id=`, returns
/// `{ current, closed_periods }` for that one asset — `closed_periods` comes
/// from WP1.6's monthly `AssetLedger` rollover archive, and the Dashboard's
/// ledger card lists them.
pub async fn get_ledger(
    State(state): State<AppState>,
    State(history): State<History>,
    Query(params): Query<LedgerQuery>,
) -> impl IntoResponse {
    let current = state.asset_ledger().await;
    let Some(asset_id) = params.asset_id else {
        return Json(current).into_response();
    };

    let closed_periods = match history {
        Some(history) => {
            let asset_id = asset_id.clone();
            match super::history::run_query(move || history.query_ledger_periods(&asset_id)).await {
                Ok(periods) => Some(periods),
                Err(response) => return response,
            }
        }
        None => None,
    };
    Json(AssetLedgerHistory {
        current: current.get(&asset_id).cloned(),
        closed_periods,
    })
    .into_response()
}

/// GET /plan/events — Server-Sent Events stream of planner progress.
///
/// Pushes `solving_started`, `solving_progress` (1 s ticks), and `plan_ready`
/// events so the UI can show live solver feedback.
pub async fn get_plan_events(
    State(planner_event_tx): State<PlannerEventTx>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let mut bcast_rx = planner_event_tx.subscribe();
    // Bridge broadcast → mpsc so lagged clients don't poison the broadcast sender.
    let (fwd_tx, fwd_rx) = tokio::sync::mpsc::channel::<Event>(32);
    tokio::spawn(async move {
        loop {
            match bcast_rx.recv().await {
                Ok(evt) => {
                    if let Ok(data) = serde_json::to_string(&evt) {
                        if fwd_tx.send(Event::default().data(data)).await.is_err() {
                            break; // client disconnected
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    let stream = ReceiverStream::new(fwd_rx).map(Ok::<_, std::convert::Infallible>);
    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[cfg(test)]
mod signals_wire_tests {
    use super::*;

    /// R-133: the typed body is the `json!()` it replaced, key for key.
    #[test]
    fn signals_state_keeps_the_wire_it_replaced() {
        let capacity = crate::entities::capacity::OadrCapacityState::default();
        let body = SignalsState {
            alerts: vec![],
            simple: vec![],
            dispatch: vec![],
            capacity: capacity.clone(),
            reservation_request: None,
        };
        assert_eq!(
            serde_json::to_value(&body).unwrap(),
            serde_json::json!({
                "alerts": [],
                "simple": [],
                "dispatch": [],
                "capacity": capacity,
                "reservation_request": null,
            })
        );
    }
}

#[cfg(test)]
mod ledger_tests {
    use super::*;
    use crate::controller::HistoryPort;
    use crate::services::test_support::mock_history_port::MockHistoryPort;
    use chrono::{TimeZone, Utc};
    use std::sync::Arc;

    fn september(asset_id: &str) -> LedgerPeriod {
        LedgerPeriod {
            asset_id: asset_id.into(),
            period_start: Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap(),
            period_end: Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0).unwrap(),
            energy_kwh: 42.5,
            cost_eur: 9.75,
            co2_kg: 3.2,
        }
    }

    async fn state_with_open_ev_period() -> AppState {
        let state = AppState::new();
        state
            .account_tick(|ledger, _| {
                ledger.insert("ev".into(), Default::default());
            })
            .await;
        state
    }

    async fn ledger_of(history: History, asset_id: &str) -> (StatusCode, serde_json::Value) {
        let resp = get_ledger(
            State(state_with_open_ev_period().await),
            State(history),
            Query(LedgerQuery {
                asset_id: Some(asset_id.into()),
            }),
        )
        .await
        .into_response();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn get_ledger_with_asset_id_returns_the_open_period_and_the_closed_ones() {
        let store = Arc::new(MockHistoryPort::new());
        store.append_ledger_period(&september("ev")).unwrap();
        store.append_ledger_period(&september("battery")).unwrap();

        let (status, body) = ledger_of(Some(store), "ev").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["current"]["asset_id"], "");
        assert_eq!(
            body["closed_periods"],
            serde_json::json!([september("ev")]),
            "only the asked asset's periods"
        );
    }

    /// R-136: a store that cannot be read is an error, not "no closed periods".
    #[tokio::test]
    async fn get_ledger_reports_a_store_that_cannot_be_read() {
        let store = Arc::new(MockHistoryPort::new());
        store.set_fail_storage(true);

        let (status, body) = ledger_of(Some(store), "ev").await;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(body["error"].is_string(), "{body}");
    }

    /// Without a history store the closed periods are unknown (`null`), which is not the same
    /// as there being none (`[]`).
    #[tokio::test]
    async fn get_ledger_without_a_history_store_says_closed_periods_are_unknown() {
        let (status, body) = ledger_of(None, "ev").await;

        assert_eq!(status, StatusCode::OK);
        assert!(body["current"].is_object());
        assert!(body["closed_periods"].is_null(), "{body}");
    }
}

#[cfg(test)]
mod objective_route_tests {
    use super::*;
    use crate::entities::asset::{PlanTrigger, PlanTriggerSignal};
    use crate::entities::objective_setting::{ObjectiveSetting, ObjectiveSource};
    use axum::response::IntoResponse;
    use std::sync::Arc;

    /// The VEN UI's change is applied with its source, replans, and reads back in full.
    #[tokio::test]
    async fn put_then_get_plan_objective_reports_the_change_and_its_source() {
        let setting: ActiveObjective = Arc::new(tokio::sync::RwLock::new(
            ObjectiveSetting::from_profile(PlannerObjective::MinCost),
        ));
        let (tx, mut rx) =
            tokio::sync::watch::channel(PlanTriggerSignal::bare(PlanTrigger::Periodic));
        let tx = Arc::new(tx);
        rx.mark_unchanged();
        let status = put_plan_objective(
            State(setting.clone()),
            State(tx.clone()),
            Json(SetObjectiveBody {
                objective: PlannerObjective::Opportunistic,
            }),
        )
        .await
        .into_response()
        .status();
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(rx.has_changed().unwrap(), "the change replans");

        let body = get_plan_objective(State(setting)).await.into_response();
        let bytes = axum::body::to_bytes(body.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["objective"], "opportunistic");
        assert_eq!(
            json["source"],
            serde_json::to_value(ObjectiveSource::VenUi).unwrap()
        );
        assert_eq!(json["profile_objective"], "min_cost");
        assert_eq!(
            json["available_objectives"].as_array().unwrap().len(),
            PlannerObjective::ALL.len()
        );
        assert!(json["set_at"].is_string());
    }
}
