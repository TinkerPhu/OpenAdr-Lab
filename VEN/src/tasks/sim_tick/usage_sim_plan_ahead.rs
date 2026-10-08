//! `ev-usage-simulation` plan-ahead: offers the EV's predicted leave instants to the planner
//! in advance, as simulated-origin `EvSession`s - reusing the existing session/MILP-deadline
//! mechanism (design.md Decision 5) rather than adding a second notion of "EV deadline".
//!
//! Scheduling only. The EV predicts the sessions (`Asset::planned_usage_sessions`) and the
//! application installs them (`services::ev_usage_plan`); this task asks the one and hands
//! the answer to the other every tick.

use chrono::{DateTime, Duration, Utc};

use crate::ids::ASSET_EV;
use crate::services::ev_usage_plan::install_planned_sessions;
use crate::simulator::SimState;
use crate::state::AppState;

/// How far ahead the simulated schedule keeps sessions queued.
///
/// A week: long enough that the planner sees a realistic sequence of departures
/// rather than only the next one, short enough that the queue stays small (the
/// generator produces at most one trip per calendar day, so at most seven).
/// Topped up every tick, so the span rolls rather than being filled once.
const ROLLING_WINDOW_DAYS: i64 = 7;

/// Keeps a rolling week of simulated-origin `EvSession`s queued, one per predicted trip.
/// A no-op when the EV has no plan-ahead schedule.
///
/// Deliberately not bounded by the planner's horizon: the queue is what the planner
/// reads *from*, and it outliving the horizon is the point - sessions beyond it
/// simply contribute no obligation this cycle. Taking `plan_horizon_h` here also
/// made this function disagree with the cycle task about what the horizon is
/// (R-91), a mismatch that now cannot arise.
pub(crate) async fn sync_plan_ahead_session(state: &AppState, sim: &SimState, now: DateTime<Utc>) {
    let Some((_, ev)) = sim.find_asset(ASSET_EV) else {
        return;
    };
    let planned = ev.planned_usage_sessions(now, Duration::days(ROLLING_WINDOW_DAYS));
    install_planned_sessions(state, planned).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::asset_params::{
        AssetParams, EvParams, EvUsageDayParams, EvUsageMode, EvUsageSimParams,
    };
    use crate::entities::device_session::EvSessionOrigin;
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

    fn sim_with_plan_ahead() -> SimState {
        let params = EvParams {
            usage_sim: Some(EvUsageSimParams {
                mode: EvUsageMode::Simulated,
                engage_charge_planning: true,
                weekday: always_leaves_at(8),
                weekend: always_leaves_at(8),
                min_soc_after_drop_pct: 5.0,
            }),
            ..EvParams::default()
        };
        SimState::from_params(
            &[AssetParams::Ev(params)],
            Utc.with_ymd_and_hms(2026, 7, 20, 0, 0, 0).unwrap(),
        )
    }

    /// The wiring: what the EV predicts reaches the queue, and a second tick adds nothing.
    /// What is predicted is tested at the EV (`ev_schedule`), how it is installed in
    /// `services::ev_usage_plan`.
    #[tokio::test]
    async fn a_tick_queues_the_evs_predicted_week_once() {
        let state = AppState::new();
        let sim = sim_with_plan_ahead();
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 6, 0, 0).unwrap();

        sync_plan_ahead_session(&state, &sim, now).await;
        let first: Vec<_> = state.ev_sessions().await.iter().map(|s| s.id).collect();
        sync_plan_ahead_session(&state, &sim, now).await;
        let second: Vec<_> = state.ev_sessions().await.iter().map(|s| s.id).collect();

        assert_eq!(first.len(), 7, "one per day of the rolling week");
        assert_eq!(first, second, "a second tick must change nothing");
        assert!(state
            .ev_sessions()
            .await
            .iter()
            .all(|s| s.origin == EvSessionOrigin::SimulatedUsage));
    }

    #[tokio::test]
    async fn a_roster_without_an_ev_queues_nothing() {
        let state = AppState::new();
        let sim = SimState::from_params(&[], Utc::now());
        sync_plan_ahead_session(&state, &sim, Utc::now()).await;
        assert!(state.ev_sessions().await.is_empty());
    }
}
