//! `ev-usage-simulation` plan-ahead: offers the EV's next simulated leave
//! instant to the planner in advance, by writing a simulated-origin
//! `EvSession` — reusing the existing single-`EvSession`/MILP-deadline
//! mechanism (design.md Decision 5) rather than adding a second, parallel
//! notion of "EV deadline". Split out of `tick.rs` to stay under the
//! tasks/ file-size cap.

use chrono::{DateTime, Duration, Utc};

use crate::assets::ev::EvCharger;
use crate::assets::ev_schedule::next_trip_after;
use crate::entities::asset_params::EvUsageMode;
use crate::entities::device_session::{EvSession, EvSessionOrigin};
use crate::ids::ASSET_EV;
use crate::simulator::SimState;
use crate::state::AppState;

/// How far ahead the simulated schedule keeps sessions queued.
///
/// A week: long enough that the planner sees a realistic sequence of departures
/// rather than only the next one, short enough that the queue stays small (the
/// generator produces at most one trip per calendar day, so at most seven).
/// Topped up every tick, so the span rolls rather than being filled once.
const ROLLING_WINDOW_DAYS: i64 = 7;

/// Keeps a rolling week of simulated-origin `EvSession`s queued, one per predicted
/// trip, so the planner sees a sequence of departures rather than only the next.
/// A no-op when plan-ahead is disabled or unconfigured, or under the forecast usage
/// class (which hands the planner its deadline directly, writing no session).
///
/// Deliberately not bounded by the planner's horizon: the queue is what the planner
/// reads *from*, and it outliving the horizon is the point - sessions beyond it
/// simply contribute no obligation this cycle. Taking `plan_horizon_h` here also
/// made this function disagree with the cycle task about what the horizon is
/// (R-91), a mismatch that now cannot arise.
pub(crate) async fn sync_plan_ahead_session(state: &AppState, sim: &SimState, now: DateTime<Utc>) {
    let Some((_, cfg)) = sim.find_asset(ASSET_EV) else {
        return;
    };
    let Some(ev) = cfg.as_any().downcast_ref::<EvCharger>() else {
        return;
    };
    let Some(usage_sim) = &ev.usage_sim else {
        return;
    };
    if !usage_sim.engage_charge_planning {
        return;
    }
    // `ev-usage-forecast` carries the predicted deadline into the MILP directly
    // (`EvMilpContext::apply_usage_forecast`), so writing a session here would be
    // a second, competing copy of the same goal. Session-writing is the
    // `usage_sim` class's mechanism alone.
    if usage_sim.mode != EvUsageMode::Simulated {
        return;
    }

    // A rolling week, not just the next trip. The generator is a pure function of
    // (config, day, seed), so "the schedule" is already infinite and reproducible -
    // what was missing was somewhere to put more than one of it.
    let horizon_end = now + Duration::days(ROLLING_WINDOW_DAYS);

    // Each trip's session may charge from when the car got home - the previous
    // trip's return - up to this trip's departure. The first one starts now: the car
    // is either home already or mid-trip, and either way now is when charging may
    // begin. Walking the trips in order is what lets each session's window start at
    // the previous return without a second notion of "when is the car home".
    let mut cursor = now;
    let mut window_open = now;
    while let Some(trip) = next_trip_after(usage_sim, ev.usage_sim_seed_tag, cursor, horizon_end) {
        let session = EvSession {
            id: uuid::Uuid::new_v4(),
            target_soc: ev.soc_target_profile,
            window_start: window_open.max(now),
            // The generated trip already states its own consumption as a SoC
            // percentage, so this session needs no distance: `soc_drop_pct` is
            // carried straight through below rather than round-tripped through
            // kilometres and back, which would only invite the two to disagree.
            expected_trip_distance_km: None,
            departure_time: trip.leave_at,
            soft_deadline: false,
            mode: Default::default(),
            origin: EvSessionOrigin::SimulatedUsage,
            budget_eur: None,
            comfort_rates: vec![],
            created_at: now,
            updated_at: now,
        };

        // Idempotent by construction: a tick that re-derives a trip already queued
        // must change nothing, and this runs every tick.
        let already_queued = state
            .ev_sessions()
            .await
            .iter()
            .any(|s| s.departure_time == trip.leave_at);
        if !already_queued {
            // A conflict means a stated session already covers this window. The
            // simulated schedule yields - it stands in for a user, so it never
            // displaces one - and the remaining trips are still placed around it.
            if let Err(conflict) = state.insert_ev_session(session).await {
                tracing::debug!(
                    leave_at = %trip.leave_at,
                    // Ids, not whole sessions: the clash carries the sessions so a prompt can
                    // describe them, but a log line wants the identity, not the payload.
                    conflicts = ?conflict.conflicts.iter().map(|s| s.id).collect::<Vec<_>>(),
                    "simulated EV session skipped: a stated session covers this window"
                );
            }
        }

        window_open = trip.return_at;
        cursor = trip.leave_at;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::asset_params::{EvParams, EvUsageDayParams, EvUsageSimParams};
    use chrono::{NaiveTime, TimeZone};

    fn always_leaves_at(hour: u32) -> EvUsageDayParams {
        EvUsageDayParams {
            leave_time: NaiveTime::from_hms_opt(hour, 0, 0).unwrap(),
            leave_jitter_min: 0.0,
            return_time: NaiveTime::from_hms_opt((hour + 8) % 24, 0, 0).unwrap(),
            return_jitter_min: 0.0,
            leave_probability: 1.0,
            soc_drop_pct_mean: 10.0,
            soc_drop_pct_stddev: 0.0,
        }
    }

    fn ev_params_with_plan_ahead(engage_charge_planning: bool) -> EvParams {
        EvParams {
            usage_sim: Some(EvUsageSimParams {
                mode: EvUsageMode::Simulated,
                engage_charge_planning,
                weekday: always_leaves_at(8),
                weekend: always_leaves_at(8),
                min_soc_after_drop_pct: 5.0,
            }),
            ..EvParams::default()
        }
    }

    fn sim_with(params: EvParams) -> SimState {
        SimState::from_params(
            &[crate::entities::asset_params::AssetParams::Ev(params)],
            Utc.with_ymd_and_hms(2026, 7, 20, 0, 0, 0).unwrap(),
        )
    }

    #[tokio::test]
    async fn queues_a_session_for_every_predicted_trip_in_the_rolling_week() {
        let state = AppState::new();
        // leave_probability 1.0, so this profile leaves every single day.
        let sim = sim_with(ev_params_with_plan_ahead(true));
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 6, 0, 0).unwrap();

        sync_plan_ahead_session(&state, &sim, now).await;

        let sessions = state.ev_sessions().await;
        assert_eq!(sessions.len(), 7, "one per day of the rolling week");
        assert!(sessions
            .iter()
            .all(|s| s.origin == EvSessionOrigin::SimulatedUsage));
        // The first is today's 08:00 leave; the rest follow one day apart.
        let departures: Vec<_> = sessions.iter().map(|s| s.departure_time).collect();
        assert_eq!(
            departures[0],
            Utc.with_ymd_and_hms(2026, 7, 20, 8, 0, 0).unwrap()
        );
        for pair in departures.windows(2) {
            assert_eq!(pair[1] - pair[0], Duration::days(1));
        }
    }

    /// Each session may charge from when the car got home, so a later session's
    /// window opens at the previous trip's return - not at `now`, which would claim
    /// the car is available while it is still out.
    #[tokio::test]
    async fn a_later_session_opens_its_window_at_the_previous_trips_return() {
        let state = AppState::new();
        let sim = sim_with(ev_params_with_plan_ahead(true));
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 6, 0, 0).unwrap();

        sync_plan_ahead_session(&state, &sim, now).await;

        let sessions = state.ev_sessions().await;
        let v: Vec<_> = sessions.iter().collect();
        assert_eq!(v[0].window_start, now, "the imminent one starts now");
        // The profile returns 8h after leaving, so the second window opens at 16:00.
        assert_eq!(
            v[1].window_start,
            Utc.with_ymd_and_hms(2026, 7, 20, 16, 0, 0).unwrap()
        );
    }

    /// Runs every tick, so re-deriving the same week must add nothing.
    #[tokio::test]
    async fn repeated_ticks_queue_no_duplicates() {
        let state = AppState::new();
        let sim = sim_with(ev_params_with_plan_ahead(true));
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 6, 0, 0).unwrap();

        sync_plan_ahead_session(&state, &sim, now).await;
        let first: Vec<_> = state.ev_sessions().await.iter().map(|s| s.id).collect();
        sync_plan_ahead_session(&state, &sim, now).await;
        let second: Vec<_> = state.ev_sessions().await.iter().map(|s| s.id).collect();

        assert_eq!(first, second, "a second tick must change nothing");
    }

    #[tokio::test]
    async fn writes_no_session_under_the_forecast_usage_class() {
        // `ev-usage-forecast` hands the deadline to the MILP directly; a session
        // written here would be a second copy of the same goal.
        let state = AppState::new();
        let mut params = ev_params_with_plan_ahead(true);
        params.usage_sim.as_mut().unwrap().mode = EvUsageMode::Forecast;
        let sim = sim_with(params);
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 6, 0, 0).unwrap();
        sync_plan_ahead_session(&state, &sim, now).await;
        assert!(state.ev_sessions().await.is_empty());
    }

    #[tokio::test]
    async fn does_nothing_when_plan_ahead_disabled() {
        let state = AppState::new();
        let sim = sim_with(ev_params_with_plan_ahead(false));
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 6, 0, 0).unwrap();
        sync_plan_ahead_session(&state, &sim, now).await;
        assert!(state.ev_sessions().await.is_empty());
    }

    #[tokio::test]
    async fn never_overwrites_a_real_user_session() {
        let state = AppState::new();
        let sim = sim_with(ev_params_with_plan_ahead(true));
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 6, 0, 0).unwrap();
        let real = EvSession {
            id: uuid::Uuid::new_v4(),
            target_soc: 0.95,
            window_start: now,
            expected_trip_distance_km: None,
            departure_time: now + Duration::hours(1),
            soft_deadline: false,
            mode: Default::default(),
            origin: EvSessionOrigin::UserRequest,
            budget_eur: None,
            comfort_rates: vec![],
            created_at: now,
            updated_at: now,
        };
        state.insert_ev_session(real.clone()).await.unwrap();
        sync_plan_ahead_session(&state, &sim, now).await;

        let sessions = state.ev_sessions().await;
        assert!(
            sessions.iter().any(|s| s.id == real.id),
            "the stated session must be untouched"
        );
        // It yields only where it clashes: the rest of the week is still placed.
        assert!(
            sessions.len() > 1,
            "simulated sessions must fill the gaps around it, got {}",
            sessions.len()
        );
    }

    /// A day the profile predicts no trip for simply has no session - the week is
    /// not padded to seven.
    #[tokio::test]
    async fn a_day_without_a_predicted_trip_gets_no_session() {
        let state = AppState::new();
        let mut params = ev_params_with_plan_ahead(true);
        // Never leaves: the generator rolls against this probability per day.
        params.usage_sim.as_mut().unwrap().weekday.leave_probability = 0.0;
        params.usage_sim.as_mut().unwrap().weekend.leave_probability = 0.0;
        let sim = sim_with(params);
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 6, 0, 0).unwrap();

        sync_plan_ahead_session(&state, &sim, now).await;

        assert!(state.ev_sessions().await.is_empty());
    }
}
