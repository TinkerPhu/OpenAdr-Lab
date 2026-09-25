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
use super::{Asset, AssetState, Trajectory, TrajectoryPoint};
use crate::entities::asset_params::EvUsageSimParams;

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
        s.soc = (s.soc - trip.soc_drop_pct / 100.0).max(floor);
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
        let AssetState::Ev(_) = initial else {
            unreachable!("EvCharger/state mismatch")
        };
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
            plan_ahead: false,
            weekday,
            weekend,
            min_soc_after_drop_pct: 5.0,
        }
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
}
