//! `ev-usage-simulation` plan-ahead: offers the EV's next simulated leave
//! instant to the planner in advance, by writing a simulated-origin
//! `EvSession` — reusing the existing single-`EvSession`/MILP-deadline
//! mechanism (design.md Decision 5) rather than adding a second, parallel
//! notion of "EV deadline". Split out of `tick.rs` to stay under the
//! tasks/ file-size cap.

use chrono::{DateTime, Duration, Utc};

use crate::assets::ev::EvCharger;
use crate::assets::ev_schedule::{active_trip_at, daily_trip, UsageTrip};
use crate::entities::asset_params::EvUsageSimParams;
use crate::entities::device_session::{EvSession, EvSessionOrigin};
use crate::ids::ASSET_EV;
use crate::simulator::SimState;
use crate::state::AppState;

/// Looks ahead day by day (bounded by `plan_horizon_h`) for the next
/// scheduled trip when none is active right now.
fn next_trip_within_horizon(
    cfg: &EvUsageSimParams,
    seed_tag: u64,
    now: DateTime<Utc>,
    plan_horizon_h: u64,
) -> Option<UsageTrip> {
    let horizon_end = now + Duration::hours(plan_horizon_h as i64);
    let mut day = now.date_naive();
    while day.and_time(chrono::NaiveTime::MIN).and_utc() <= horizon_end {
        if let Some(trip) = daily_trip(cfg, day, seed_tag) {
            if trip.leave_at > now && trip.leave_at <= horizon_end {
                return Some(trip);
            }
        }
        day += Duration::days(1);
    }
    None
}

/// Writes a simulated-origin `EvSession` for the EV's next scheduled leave
/// when plan-ahead is enabled, a trip falls within `plan_horizon_h`, and no
/// real (user/VTN) session is currently active. A no-op in every other case,
/// including when disabled or unconfigured — today's behavior, unchanged.
pub(crate) async fn sync_plan_ahead_session(
    state: &AppState,
    sim: &SimState,
    now: DateTime<Utc>,
    plan_horizon_h: u64,
) {
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

    let trip = active_trip_at(usage_sim, ev.usage_sim_seed_tag, now).or_else(|| {
        next_trip_within_horizon(usage_sim, ev.usage_sim_seed_tag, now, plan_horizon_h)
    });
    let Some(trip) = trip else {
        return;
    };
    if trip.leave_at > now + Duration::hours(plan_horizon_h as i64) {
        return;
    }

    let existing = state.ev_session().await;
    match &existing {
        // A real user/VTN session is active — it always wins; never touched.
        Some(s) if s.origin != EvSessionOrigin::SimulatedUsage => return,
        // Already reflects this exact trip — nothing to refresh.
        Some(s) if s.departure_time == trip.leave_at => return,
        _ => {}
    }

    state
        .set_ev_session(Some(EvSession {
            id: uuid::Uuid::new_v4(),
            target_soc: ev.soc_target_profile,
            departure_time: trip.leave_at,
            soft_deadline: false,
            mode: Default::default(),
            origin: EvSessionOrigin::SimulatedUsage,
            budget_eur: None,
            comfort_rates: vec![],
            created_at: now,
            updated_at: now,
        }))
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::asset_params::{EvParams, EvUsageDayParams};
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
    async fn writes_a_simulated_session_when_plan_ahead_enabled_and_trip_in_horizon() {
        let state = AppState::new();
        let sim = sim_with(ev_params_with_plan_ahead(true));
        // 2026-07-20 06:00 -- the day's 08:00 leave is 2h ahead, well within 48h.
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 6, 0, 0).unwrap();
        sync_plan_ahead_session(&state, &sim, now, 48).await;
        let session = state.ev_session().await.expect("session must be written");
        assert_eq!(session.origin, EvSessionOrigin::SimulatedUsage);
        assert_eq!(
            session.departure_time,
            Utc.with_ymd_and_hms(2026, 7, 20, 8, 0, 0).unwrap()
        );
    }

    #[tokio::test]
    async fn does_nothing_when_plan_ahead_disabled() {
        let state = AppState::new();
        let sim = sim_with(ev_params_with_plan_ahead(false));
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 6, 0, 0).unwrap();
        sync_plan_ahead_session(&state, &sim, now, 48).await;
        assert!(state.ev_session().await.is_none());
    }

    #[tokio::test]
    async fn never_overwrites_a_real_user_session() {
        let state = AppState::new();
        let sim = sim_with(ev_params_with_plan_ahead(true));
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 6, 0, 0).unwrap();
        let real = EvSession {
            id: uuid::Uuid::new_v4(),
            target_soc: 0.95,
            departure_time: now + Duration::hours(1),
            soft_deadline: false,
            mode: Default::default(),
            origin: EvSessionOrigin::UserRequest,
            budget_eur: None,
            comfort_rates: vec![],
            created_at: now,
            updated_at: now,
        };
        state.set_ev_session(Some(real.clone())).await;
        sync_plan_ahead_session(&state, &sim, now, 48).await;
        let after = state.ev_session().await.unwrap();
        assert_eq!(after.id, real.id, "real session must be untouched");
    }

    #[tokio::test]
    async fn refreshes_a_previously_simulated_session_for_a_new_trip() {
        let state = AppState::new();
        let sim = sim_with(ev_params_with_plan_ahead(true));
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 6, 0, 0).unwrap();
        let stale = EvSession {
            id: uuid::Uuid::new_v4(),
            target_soc: 0.8,
            departure_time: now - Duration::hours(3), // a past, stale trip
            soft_deadline: false,
            mode: Default::default(),
            origin: EvSessionOrigin::SimulatedUsage,
            budget_eur: None,
            comfort_rates: vec![],
            created_at: now,
            updated_at: now,
        };
        let stale_id = stale.id;
        state.set_ev_session(Some(stale)).await;
        sync_plan_ahead_session(&state, &sim, now, 48).await;
        let after = state.ev_session().await.unwrap();
        assert_ne!(after.id, stale_id, "must be replaced with the fresh trip");
        assert_eq!(
            after.departure_time,
            Utc.with_ymd_and_hms(2026, 7, 20, 8, 0, 0).unwrap()
        );
    }
}
