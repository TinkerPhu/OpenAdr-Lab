//! `EvCharger`'s departure-aware forward projection — split out of `ev.rs` to
//! stay under the file-size cap. `ev-departure-consolidation`: this is the
//! mechanism that makes `Asset::simulate_forward` genuinely aware that the
//! car will leave at `departure_time`, replacing the site-level
//! `capacity_headroom.rs` `ev_session` workaround this phase retires.
//!
//! `ev-usage-simulation` extends the same "when is this EV unavailable"
//! primitive with a second, independent source: a profile-configured daily
//! leave/return trip, generated deterministically per calendar day
//! (`daily_trip`) so a live tick and a multi-day forecast both ask the same
//! question the same way instead of drifting apart (one-concept-one-function).

use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rand_distr::{Distribution, Normal};

use super::ev::{EvCharger, EvState};
use super::own_state::own;
use super::{Asset, AssetState, Trajectory, TrajectoryPoint};
use crate::entities::asset_params::{EvUsageMode, EvUsageSimParams};
use crate::entities::device_session::EvSession;
use crate::entities::ev_usage::{EvUsageSimState, NextTrip};

/// One simulated day's leave/return trip, per `ev-usage-simulation`.
#[derive(Debug, Clone, PartialEq)]
pub struct UsageTrip {
    pub leave_at: DateTime<Utc>,
    pub return_at: DateTime<Utc>,
    /// State-of-charge drop consumed while away (%, 0-100), already jittered
    /// but not yet floor-clamped (clamping depends on the SoC at return, not
    /// on the trip alone).
    pub soc_drop_pct: f64,
}

impl EvCharger {
    /// What a trip of `distance_km` is expected to consume, as a fraction of pack.
    ///
    /// The EV is the only authority for this conversion
    /// (`asset-competence-assurance`): it owns both the consumption rate and the pack
    /// size, and a route, interface or planner doing its own
    /// `km × kWh/km ÷ battery_kwh` would be a second copy of the rule.
    ///
    /// Takes a stated distance, not an `Option`. There is no default to fall back to:
    /// an unstated trip projects no drop at all, decided by the caller having nothing
    /// to convert rather than by this function inventing a distance.
    pub fn expected_trip_drop_frac(&self, distance_km: f64) -> f64 {
        if self.battery_kwh <= 0.0 {
            return 0.0;
        }
        (distance_km.max(0.0) * self.consumption_kwh_per_km / self.battery_kwh).clamp(0.0, 1.0)
    }
}

/// FNV-1a over the EV's configured `id`, so two distinct EVs with identical
/// `usage_sim` config still draw independent day-to-day sequences (mirrors
/// `base_load.rs`'s per-spike `seed_tag`, just derived from a string here
/// since an EV asset has exactly one usage-sim config, not a list).
pub(super) fn usage_sim_seed_tag(id: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in id.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    hash
}

/// Today's trip (or none), deterministic for a given `(cfg, day, seed_tag)` —
/// same day always reproduces the same roll, reproducible in tests, while
/// still varying day to day (mirrors `base_load.rs::appliance_noise_kw`'s
/// seeding idiom). The weekday/weekend config for the whole trip — including
/// a midnight-crossing return — is chosen by the day the EV leaves.
pub fn daily_trip(cfg: &EvUsageSimParams, day: NaiveDate, seed_tag: u64) -> Option<UsageTrip> {
    let day_ordinal = day.num_days_from_ce() as u64;
    let seed = day_ordinal
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(seed_tag);
    let mut rng = StdRng::seed_from_u64(seed);

    let is_weekend = day.weekday().num_days_from_monday() >= 5;
    let day_cfg = if is_weekend {
        &cfg.weekend
    } else {
        &cfg.weekday
    };

    let leaves_today: f64 = rng.gen_range(0.0..1.0);
    if leaves_today > day_cfg.leave_probability {
        return None;
    }

    let leave_jitter_s =
        (rng.gen_range(-day_cfg.leave_jitter_min..=day_cfg.leave_jitter_min) * 60.0) as i64;
    let return_jitter_s =
        (rng.gen_range(-day_cfg.return_jitter_min..=day_cfg.return_jitter_min) * 60.0) as i64;

    let leave_naive = day.and_time(day_cfg.leave_time) + Duration::seconds(leave_jitter_s);
    let mut return_naive = day.and_time(day_cfg.return_time) + Duration::seconds(return_jitter_s);
    if return_naive < leave_naive {
        // Return clock-time falls before leave clock-time -- the trip crosses
        // midnight, governed throughout by the leave day's own config.
        return_naive += Duration::days(1);
    }

    // Gaussian jitter on the SoC drop: unlike leave/return timing, the drop is
    // floor-clamped downstream, so an unbounded tail is safe here (see
    // design.md Decision 2) — never below 0.0 (a "drop" cannot be negative).
    let normal = Normal::new(
        day_cfg.soc_drop_pct_mean,
        day_cfg.soc_drop_pct_stddev.max(0.0),
    )
    .unwrap_or_else(|_| Normal::new(day_cfg.soc_drop_pct_mean, 0.0).unwrap());
    let soc_drop_pct = normal.sample(&mut rng).max(0.0);

    Some(UsageTrip {
        leave_at: DateTime::<Utc>::from_naive_utc_and_offset(leave_naive, Utc),
        return_at: DateTime::<Utc>::from_naive_utc_and_offset(return_naive, Utc),
        soc_drop_pct,
    })
}

/// The trip (if any) covering `ts` — checks both `ts`'s own calendar day and
/// the previous one, since a midnight-crossing trip is generated under the
/// *previous* day's date but can still be active after midnight.
pub fn active_trip_at(
    cfg: &EvUsageSimParams,
    seed_tag: u64,
    ts: DateTime<Utc>,
) -> Option<UsageTrip> {
    let today = ts.date_naive();
    [today - Duration::days(1), today]
        .into_iter()
        .find_map(|day| {
            daily_trip(cfg, day, seed_tag).filter(|trip| ts >= trip.leave_at && ts < trip.return_at)
        })
}

/// The next trip starting strictly after `now` and no later than
/// `horizon_end`, scanning day by day. `None` when the EV is not predicted to
/// leave again inside that window.
///
/// One implementation for both callers that need "when does it next leave":
/// `usage_sim`'s session-writing path (`tasks::sim_tick::usage_sim_plan_ahead`)
/// and `usage_forecast`'s in-context deadline (`EvMilpContext`).
pub fn next_trip_after(
    cfg: &EvUsageSimParams,
    seed_tag: u64,
    now: DateTime<Utc>,
    horizon_end: DateTime<Utc>,
) -> Option<UsageTrip> {
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

/// Every trip departing after `from` and no later than `horizon_end`, each paired with the
/// instant its charging window opens: `first_window_open` for the first, the previous trip's
/// return for each later one. The one walk that both the session queue (`usage_sim`) and the
/// MILP's expected uses (`usage_forecast`) take, so "when is the car home between two trips"
/// has a single answer.
pub fn trip_windows(
    cfg: &EvUsageSimParams,
    seed_tag: u64,
    from: DateTime<Utc>,
    first_window_open: DateTime<Utc>,
    horizon_end: DateTime<Utc>,
) -> Vec<(DateTime<Utc>, UsageTrip)> {
    let mut windows = Vec::new();
    let mut cursor = from;
    let mut window_open = first_window_open;
    while let Some(trip) = next_trip_after(cfg, seed_tag, cursor, horizon_end) {
        cursor = trip.leave_at;
        let next_open = trip.return_at;
        windows.push((window_open, trip));
        window_open = next_open;
    }
    windows
}

/// The most recently ended trip at-or-before `ts`, among the two candidate
/// leave-days — used to look up the SoC drop to apply at the exact tick a
/// trip's `return_at` is reached (see `EvCharger::ended_trip_at`).
pub fn most_recently_ended_trip(
    cfg: &EvUsageSimParams,
    seed_tag: u64,
    ts: DateTime<Utc>,
) -> Option<UsageTrip> {
    let today = ts.date_naive();
    [today - Duration::days(1), today]
        .into_iter()
        .filter_map(|day| daily_trip(cfg, day, seed_tag))
        .filter(|trip| trip.return_at <= ts)
        .max_by_key(|trip| trip.return_at)
}

impl EvCharger {
    /// The simulated-origin charge sessions for every predicted trip in `[now, now + window]`,
    /// one per trip, each window opening where the previous trip returned (the first opens
    /// now: the car is home or mid-trip, and either way charging may begin now). Empty unless
    /// the profile declared the `usage_sim` class with plan-ahead engaged: `usage_forecast`
    /// hands the planner its deadline directly (`EvMilpContext::apply_usage_forecast`), so a
    /// session there would be a second, competing copy of the same goal.
    pub(super) fn planned_usage_sessions(
        &self,
        now: DateTime<Utc>,
        window: Duration,
    ) -> Vec<EvSession> {
        let Some(cfg) = self.usage_sim.as_ref() else {
            return Vec::new();
        };
        if !cfg.engage_charge_planning || cfg.mode != EvUsageMode::Simulated {
            return Vec::new();
        }
        trip_windows(cfg, self.usage_sim_seed_tag, now, now, now + window)
            .into_iter()
            .map(|(window_open, trip)| {
                EvSession::simulated(
                    self.soc_target_profile,
                    window_open.max(now),
                    trip.leave_at,
                    now,
                )
            })
            .collect()
    }
}

/// How far ahead the diagnostics view looks for the next trip. For display only,
/// independent of the planner's own (shorter) horizon.
const USAGE_VIEW_LOOKAHEAD_DAYS: i64 = 7;

impl EvCharger {
    /// The configured usage schedule as `GET /ev-usage-sim` shows it: the usage class,
    /// whether charge planning is engaged, and the trip in progress or, failing that,
    /// the next one. `None` when no schedule is configured.
    pub(super) fn usage_view(&self, now: DateTime<Utc>) -> Option<EvUsageSimState> {
        let cfg = self.usage_sim.as_ref()?;
        let trip = active_trip_at(cfg, self.usage_sim_seed_tag, now).or_else(|| {
            next_trip_after(
                cfg,
                self.usage_sim_seed_tag,
                now,
                now + Duration::days(USAGE_VIEW_LOOKAHEAD_DAYS),
            )
        });
        Some(EvUsageSimState {
            mode: cfg.mode,
            engage_charge_planning: cfg.engage_charge_planning,
            next_trip: trip.map(|t| NextTrip {
                leave_at: t.leave_at,
                return_at: t.return_at,
                expected_soc_drop_pct: t.soc_drop_pct,
            }),
        })
    }

    /// True if this EV is unavailable (away) at `ts`, from either of the two
    /// independent sources: a live `EvSession` deadline (existing
    /// `ev-departure-consolidation` behavior, unbounded from `departure_time`
    /// onward) or a currently-active simulated usage trip (bounded to
    /// `[leave_at, return_at)`). One shared predicate so a live tick and a
    /// multi-day forecast can never disagree about "is this EV here."
    pub(super) fn is_away_at(&self, ts: DateTime<Utc>) -> bool {
        let via_session = self.departure_time.is_some_and(|d| ts >= d);
        let via_trip = self
            .usage_sim
            .as_ref()
            .is_some_and(|cfg| active_trip_at(cfg, self.usage_sim_seed_tag, ts).is_some());
        via_session || via_trip
    }

    /// The usage-sim trip that ended at-or-before `ts`, if any — looked up
    /// (not cached) so the drop can be applied exactly once at the tick the
    /// EV actually returns, in both the live tick and the forecast walk.
    pub(super) fn ended_trip_at(&self, ts: DateTime<Utc>) -> Option<UsageTrip> {
        self.usage_sim
            .as_ref()
            .and_then(|cfg| most_recently_ended_trip(cfg, self.usage_sim_seed_tag, ts))
    }

    /// Applies a trip's SoC drop, floored at the configured minimum. Called
    /// exactly once per trip, at the tick its `return_at` is reached, before
    /// `plugged` is set back to `true`.
    pub(super) fn apply_return_drop(&self, s: &mut EvState, trip: &UsageTrip) {
        let floor = self
            .usage_sim
            .as_ref()
            .map(|cfg| cfg.min_soc_after_drop_pct)
            .unwrap_or(0.0)
            / 100.0;
        s.soc_frac = (s.soc_frac - trip.soc_drop_pct / 100.0).max(floor);
    }

    /// The live tick's `TickOverridable::apply_tick_overrides` EV handling for
    /// `ev-usage-simulation` — split out to keep `ev.rs` under the file-size
    /// cap. Applies the SoC drop exactly once, at the tick the EV transitions
    /// from away back to plugged (before `plugged` is set true), then decides
    /// `plugged` itself: an explicit override always wins, otherwise the
    /// usage-sim schedule decides (defaulting to plugged when unconfigured —
    /// today's behavior, unchanged). Without the override's snap-back,
    /// releasing the inject would leave the EV permanently unplugged since
    /// nothing else re-plugs it.
    pub(super) fn apply_usage_sim_tick(
        &self,
        s: &mut EvState,
        now: DateTime<Utc>,
        plugged_override: Option<bool>,
    ) {
        let away_now = self.is_away_at(now);
        if s.was_away_by_usage_sim && !away_now {
            if let Some(trip) = self.ended_trip_at(now) {
                self.apply_return_drop(s, &trip);
            }
        }
        s.was_away_by_usage_sim = away_now;
        s.plugged = plugged_override.unwrap_or(!away_now);
    }

    /// Forces `plugged=false` for any trajectory point the EV is away at
    /// (`is_away_at`) before delegating to `step()` — the one thing the
    /// trait default's step()-based walk can't express, since `step()` itself
    /// is genuinely setpoint+duration-driven (no timestamp needed for EV's
    /// own physics, unlike PV's `simulate_forward` override). Also applies
    /// the usage-sim SoC drop exactly once, at the point the EV transitions
    /// from away back to plugged. `capability_inner`/`step_inner` already
    /// correctly zero out an unplugged EV, so no other asset-specific branch
    /// is needed anywhere downstream (unlike PV, whose own
    /// `max_effort_setpoint` ignores `state` entirely — see that method's
    /// doc comment).
    ///
    /// `step_inner`'s BL-12 response delay applies the *previous* step's
    /// command. That is one 1 s tick live, but a projection window is 60 s to
    /// 15 min, so each window's command is staged (a zero-length step) before
    /// it is integrated; otherwise every command would land one window late.
    pub(super) fn simulate_forward_inner(
        &self,
        initial: &AssetState,
        setpoints: &[(DateTime<Utc>, f64)],
    ) -> Trajectory {
        let _: &EvState = own(initial);
        let mut state = initial.clone();
        let mut points = Vec::new();
        let mut was_away = false;
        let mut apply_availability = |state: &mut AssetState, ts: DateTime<Utc>| {
            let away_now = self.is_away_at(ts);
            if let AssetState::Ev(s) = state {
                if was_away && !away_now {
                    if let Some(trip) = self.ended_trip_at(ts) {
                        self.apply_return_drop(s, &trip);
                    }
                }
                s.plugged = !away_now;
            }
            was_away = away_now;
        };
        let stage = |state: &AssetState, sp: f64| self.step(state, sp, Duration::zero()).0;
        for window in setpoints.windows(2) {
            let (ts, sp) = window[0];
            let dt = window[1].0 - ts;
            apply_availability(&mut state, ts);
            let (next, actual_kw) = self.step(&stage(&state, sp), sp, dt);
            points.push(TrajectoryPoint {
                ts,
                power_kw: actual_kw,
                state: state.clone(),
            });
            state = next;
        }
        if let Some(&(ts, sp)) = setpoints.last() {
            apply_availability(&mut state, ts);
            let (_, actual_kw) = self.step(&stage(&state, sp), sp, Duration::zero());
            points.push(TrajectoryPoint {
                ts,
                power_kw: actual_kw,
                state,
            });
        }
        Trajectory { points }
    }
}

#[cfg(test)]
mod usage_sim_tests {
    use super::*;
    use crate::entities::asset_params::EvUsageDayParams;
    use chrono::{NaiveTime, TimeZone, Timelike};

    fn day_cfg(leave_h: u32, return_h: u32, probability: f64) -> EvUsageDayParams {
        EvUsageDayParams {
            leave_time: NaiveTime::from_hms_opt(leave_h, 0, 0).unwrap(),
            leave_jitter_min: 10.0,
            return_time: NaiveTime::from_hms_opt(return_h, 0, 0).unwrap(),
            return_jitter_min: 10.0,
            leave_probability: probability,
            soc_drop_pct_mean: 20.0,
            soc_drop_pct_stddev: 3.0,
        }
    }

    fn usage_cfg(weekday: EvUsageDayParams, weekend: EvUsageDayParams) -> EvUsageSimParams {
        EvUsageSimParams {
            mode: crate::entities::asset_params::EvUsageMode::Simulated,
            engage_charge_planning: false,
            weekday,
            weekend,
            min_soc_after_drop_pct: 5.0,
        }
    }

    fn ev_with_schedule(usage_sim: Option<EvUsageSimParams>) -> EvCharger {
        EvCharger::from_params(&crate::entities::asset_params::EvParams {
            usage_sim,
            ..Default::default()
        })
    }

    fn plan_ahead_cfg(mode: EvUsageMode, engage: bool, probability: f64) -> EvUsageSimParams {
        let exact = |probability| EvUsageDayParams {
            leave_jitter_min: 0.0,
            return_jitter_min: 0.0,
            soc_drop_pct_stddev: 0.0,
            ..day_cfg(8, 16, probability)
        };
        let mut cfg = usage_cfg(exact(probability), exact(probability));
        cfg.mode = mode;
        cfg.engage_charge_planning = engage;
        cfg
    }

    fn monday_6am() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 20, 6, 0, 0).unwrap()
    }

    #[test]
    fn trip_windows_open_each_window_at_the_previous_return() {
        let cfg = plan_ahead_cfg(EvUsageMode::Simulated, true, 1.0);
        let now = monday_6am();
        let windows = trip_windows(&cfg, 7, now, now, now + Duration::days(3));
        assert_eq!(windows.len(), 3, "one trip per day");
        assert_eq!(
            windows[0].0, now,
            "the first window opens at the caller's instant"
        );
        for pair in windows.windows(2) {
            assert_eq!(
                pair[1].0, pair[0].1.return_at,
                "a window opens when the car is back"
            );
        }
    }

    #[test]
    fn trip_windows_is_empty_when_no_trip_is_predicted() {
        let cfg = plan_ahead_cfg(EvUsageMode::Simulated, true, 0.0);
        let now = monday_6am();
        assert!(trip_windows(&cfg, 7, now, now, now + Duration::days(7)).is_empty());
    }

    #[test]
    fn planned_usage_sessions_has_one_session_per_predicted_trip_in_the_window() {
        let ev = ev_with_schedule(Some(plan_ahead_cfg(EvUsageMode::Simulated, true, 1.0)));
        let now = monday_6am();
        let sessions = ev.planned_usage_sessions(now, Duration::days(7));
        assert_eq!(sessions.len(), 7, "one per day of the rolling week");
        assert!(sessions
            .iter()
            .all(|s| s.origin == crate::entities::device_session::EvSessionOrigin::SimulatedUsage));
        assert_eq!(
            sessions[0].departure_time,
            Utc.with_ymd_and_hms(2026, 7, 20, 8, 0, 0).unwrap(),
            "the first is today's 08:00 leave"
        );
        for pair in sessions.windows(2) {
            assert_eq!(
                pair[1].departure_time - pair[0].departure_time,
                Duration::days(1)
            );
        }
    }

    /// Each session may charge from when the car got home, so a later session's window opens at
    /// the previous trip's return - not at `now`, which would claim the car is available while
    /// it is still out.
    #[test]
    fn planned_usage_sessions_open_a_later_window_at_the_previous_return() {
        let ev = ev_with_schedule(Some(plan_ahead_cfg(EvUsageMode::Simulated, true, 1.0)));
        let now = monday_6am();
        let sessions = ev.planned_usage_sessions(now, Duration::days(7));
        assert_eq!(sessions[0].window_start, now, "the imminent one starts now");
        assert_eq!(
            sessions[1].window_start,
            Utc.with_ymd_and_hms(2026, 7, 20, 16, 0, 0).unwrap(),
            "the profile returns at 16:00"
        );
    }

    #[test]
    fn planned_usage_sessions_are_empty_under_the_forecast_usage_class() {
        // `usage_forecast` hands the deadline to the MILP directly; a session here would be a
        // second copy of the same goal.
        let ev = ev_with_schedule(Some(plan_ahead_cfg(EvUsageMode::Forecast, true, 1.0)));
        assert!(ev
            .planned_usage_sessions(monday_6am(), Duration::days(7))
            .is_empty());
    }

    #[test]
    fn planned_usage_sessions_are_empty_when_plan_ahead_is_disabled() {
        let ev = ev_with_schedule(Some(plan_ahead_cfg(EvUsageMode::Simulated, false, 1.0)));
        assert!(ev
            .planned_usage_sessions(monday_6am(), Duration::days(7))
            .is_empty());
    }

    #[test]
    fn planned_usage_sessions_are_empty_without_a_schedule_or_predicted_trip() {
        assert!(ev_with_schedule(None)
            .planned_usage_sessions(monday_6am(), Duration::days(7))
            .is_empty());
        let never = ev_with_schedule(Some(plan_ahead_cfg(EvUsageMode::Simulated, true, 0.0)));
        assert!(never
            .planned_usage_sessions(monday_6am(), Duration::days(7))
            .is_empty());
    }

    #[test]
    fn usage_view_is_none_without_a_configured_schedule() {
        assert_eq!(ev_with_schedule(None).usage_view(Utc::now()), None);
    }

    #[test]
    fn usage_view_reports_the_trip_in_progress() {
        let cfg = usage_cfg(day_cfg(8, 17, 1.0), day_cfg(8, 17, 1.0));
        let ev = ev_with_schedule(Some(cfg.clone()));
        let noon = Utc.with_ymd_and_hms(2026, 7, 20, 12, 0, 0).unwrap();
        let view = ev.usage_view(noon).expect("a schedule is configured");
        assert_eq!(view.mode, cfg.mode);
        assert_eq!(view.engage_charge_planning, cfg.engage_charge_planning);
        let trip = view
            .next_trip
            .expect("the day's trip is in progress at noon");
        assert!(trip.leave_at <= noon && noon < trip.return_at);
    }

    #[test]
    fn usage_view_looks_ahead_to_the_next_trip_when_home() {
        let cfg = usage_cfg(day_cfg(8, 17, 1.0), day_cfg(8, 17, 1.0));
        let ev = ev_with_schedule(Some(cfg));
        let evening = Utc.with_ymd_and_hms(2026, 7, 20, 22, 0, 0).unwrap();
        let trip = ev
            .usage_view(evening)
            .and_then(|v| v.next_trip)
            .expect("tomorrow's trip is inside the look-ahead");
        assert!(trip.leave_at > evening);
    }

    #[test]
    fn usage_view_has_no_next_trip_when_none_is_ever_scheduled() {
        let cfg = usage_cfg(day_cfg(8, 17, 0.0), day_cfg(8, 17, 0.0));
        let ev = ev_with_schedule(Some(cfg));
        let view = ev
            .usage_view(Utc::now())
            .expect("the schedule is configured");
        assert_eq!(view.next_trip, None);
    }

    // 2026-07-20 is a Monday.
    fn monday() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 7, 20).unwrap()
    }
    fn saturday() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 7, 25).unwrap()
    }

    #[test]
    fn daily_trip_is_deterministic_for_the_same_day() {
        let cfg = usage_cfg(day_cfg(8, 17, 1.0), day_cfg(10, 18, 1.0));
        let a = daily_trip(&cfg, monday(), 42);
        let b = daily_trip(&cfg, monday(), 42);
        assert_eq!(a, b);
    }

    #[test]
    fn daily_trip_varies_by_seed_tag() {
        let cfg = usage_cfg(day_cfg(8, 17, 1.0), day_cfg(10, 18, 1.0));
        let a = daily_trip(&cfg, monday(), 1).unwrap();
        let b = daily_trip(&cfg, monday(), 2).unwrap();
        assert_ne!(
            a.leave_at, b.leave_at,
            "distinct seed tags must not lock-step"
        );
    }

    #[test]
    fn daily_trip_probability_zero_never_leaves() {
        let cfg = usage_cfg(day_cfg(8, 17, 0.0), day_cfg(10, 18, 0.0));
        for offset in 0..30 {
            let day = monday() + Duration::days(offset);
            assert!(daily_trip(&cfg, day, 7).is_none(), "day offset {offset}");
        }
    }

    #[test]
    fn daily_trip_probability_one_always_leaves() {
        let cfg = usage_cfg(day_cfg(8, 17, 1.0), day_cfg(10, 18, 1.0));
        for offset in 0..30 {
            let day = monday() + Duration::days(offset);
            assert!(daily_trip(&cfg, day, 7).is_some(), "day offset {offset}");
        }
    }

    #[test]
    fn daily_trip_weekend_uses_weekend_config() {
        let cfg = usage_cfg(day_cfg(8, 17, 1.0), day_cfg(10, 18, 1.0));
        let trip = daily_trip(&cfg, saturday(), 7).unwrap();
        let expected_hour = trip.leave_at.time().hour();
        // Weekend nominal leave is 10:00 +/- 10 min jitter -> hour is 9 or 10.
        assert!(
            (9..=10).contains(&expected_hour),
            "expected weekend leave hour near 10, got {expected_hour}"
        );
    }

    #[test]
    fn daily_trip_jitter_stays_within_configured_bounds() {
        let cfg = usage_cfg(day_cfg(8, 17, 1.0), day_cfg(10, 18, 1.0));
        for seed in 0..50u64 {
            let trip = daily_trip(&cfg, monday(), seed).unwrap();
            let nominal_leave = Utc.with_ymd_and_hms(2026, 7, 20, 8, 0, 0).unwrap();
            let delta_min = (trip.leave_at - nominal_leave).num_seconds() as f64 / 60.0;
            assert!(
                (-10.0..=10.0).contains(&delta_min),
                "leave jitter out of bounds: {delta_min} min (seed {seed})"
            );
        }
    }

    #[test]
    fn daily_trip_midnight_crossing_rolls_return_to_next_day() {
        // return_time (02:00) is numerically before leave_time (22:00) -- the
        // trip must cross midnight, landing on the following calendar day.
        let cfg = usage_cfg(day_cfg(22, 2, 1.0), day_cfg(22, 2, 1.0));
        let trip = daily_trip(&cfg, monday(), 7).unwrap();
        assert!(trip.return_at > trip.leave_at, "return must be after leave");
        assert_eq!(
            trip.return_at.date_naive(),
            monday() + Duration::days(1),
            "return must land on the day after the leave day"
        );
    }

    #[test]
    fn active_trip_at_covers_a_midnight_crossing_window() {
        let cfg = usage_cfg(day_cfg(22, 2, 1.0), day_cfg(22, 2, 1.0));
        let trip = daily_trip(&cfg, monday(), 7).unwrap();
        let just_after_midnight = trip.leave_at + Duration::hours(3); // well past midnight, before return
        assert!(just_after_midnight < trip.return_at);
        let found = active_trip_at(&cfg, 7, just_after_midnight);
        assert_eq!(found, Some(trip));
    }

    #[test]
    fn soc_drop_never_negative() {
        let mut cfg = usage_cfg(day_cfg(8, 17, 1.0), day_cfg(10, 18, 1.0));
        cfg.weekday.soc_drop_pct_mean = 0.5;
        cfg.weekday.soc_drop_pct_stddev = 5.0; // large stddev relative to mean
        for seed in 0..100u64 {
            let trip = daily_trip(&cfg, monday(), seed).unwrap();
            assert!(
                trip.soc_drop_pct >= 0.0,
                "seed {seed}: {}",
                trip.soc_drop_pct
            );
        }
    }

    // ── expected_trip_drop_frac: the EV's own distance-to-SoC conversion ─────
    //
    // The "falls back to the configured default" test is deleted, not adapted: the
    // fallback itself is gone. An unstated trip now projects nothing, which is tested
    // where that decision is made (ev_trip_series: a use with no consumption) rather
    // than here, where there is no longer an unstated case to convert.

    fn ev_with(consumption_kwh_per_km: f64, battery_kwh: f64) -> EvCharger {
        EvCharger::from_params(&crate::entities::asset_params::EvParams {
            battery_kwh,
            consumption_kwh_per_km,
            ..Default::default()
        })
    }

    #[test]
    fn expected_trip_drop_frac_scales_with_consumption_and_pack() {
        // 120 km at 0.2 kWh/km = 24 kWh; on a 60 kWh pack that is 40 %.
        let ev = ev_with(0.2, 60.0);
        assert!((ev.expected_trip_drop_frac(120.0) - 0.40).abs() < 1e-9);
    }

    #[test]
    fn expected_trip_drop_frac_cannot_exceed_a_full_pack() {
        let ev = ev_with(0.2, 60.0);
        assert_eq!(ev.expected_trip_drop_frac(5000.0), 1.0);
    }

    #[test]
    fn expected_trip_drop_frac_treats_a_negative_distance_as_none() {
        let ev = ev_with(0.2, 60.0);
        assert_eq!(ev.expected_trip_drop_frac(-10.0), 0.0);
    }

    #[test]
    fn expected_trip_drop_frac_is_zero_without_a_pack_size() {
        let ev = ev_with(0.2, 0.0);
        assert_eq!(ev.expected_trip_drop_frac(100.0), 0.0);
    }
}
