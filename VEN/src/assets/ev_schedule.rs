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

/// Per-slot availability across a planning horizon (`ev-usage-forecast`):
/// `false` for any slot whose start falls inside a predicted trip's
/// `[leave_at, return_at)`, `true` otherwise.
///
/// `cum_s[t]` is slot `t`'s start as seconds from `now` — the same array the
/// MILP context builder already receives, so the planner's own grid decides the
/// resolution. Multiple trips inside one horizon need no special handling: each
/// slot is asked independently, so any number of away-windows simply appear as
/// more `false` runs in the returned vector.
pub fn availability_per_slot(
    cfg: &EvUsageSimParams,
    seed_tag: u64,
    now: DateTime<Utc>,
    cum_s: &[i64],
    n: usize,
) -> Vec<bool> {
    (0..n)
        .map(|t| {
            let ts = now + Duration::seconds(cum_s.get(t).copied().unwrap_or(0));
            active_trip_at(cfg, seed_tag, ts).is_none()
        })
        .collect()
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

/// Per-slot exogenous state-of-charge drop, as a SoC fraction, across a
/// planning horizon (`ev-usage-forecast`). Zero everywhere except the first
/// slot at or after a predicted trip's `return_at` — the same rule the live
/// tick applies ("first tick at or after the return instant"), so the plan's
/// projected SoC and the simulation agree about when the drop lands.
///
/// Slot 0 never carries a drop: a return that has already happened is already
/// reflected in the live SoC the plan starts from, and re-applying it here
/// would double-count.
///
/// Note: if two trips ended inside one slot, only the later one's drop is
/// counted. That needs slots longer than a day, which no planning grid uses.
pub fn soc_drop_frac_per_slot(
    cfg: &EvUsageSimParams,
    seed_tag: u64,
    now: DateTime<Utc>,
    cum_s: &[i64],
    n: usize,
) -> Vec<f64> {
    let mut drops = vec![0.0; n];
    for (t, drop) in drops.iter_mut().enumerate().take(n).skip(1) {
        let slot_start = now + Duration::seconds(cum_s.get(t).copied().unwrap_or(0));
        let prev_start = now + Duration::seconds(cum_s.get(t - 1).copied().unwrap_or(0));
        if let Some(trip) = most_recently_ended_trip(cfg, seed_tag, slot_start) {
            if trip.return_at > prev_start {
                *drop = trip.soc_drop_pct / 100.0;
            }
        }
    }
    drops
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
            mode: crate::entities::asset_params::EvUsageMode::Simulated,
            engage_charge_planning: false,
            weekday,
            weekend,
            min_soc_after_drop_pct: 5.0,
        }
    }

    /// Hourly slot grid: `cum_s[t] = t * 3600`.
    fn hourly_slots(n: usize) -> Vec<i64> {
        (0..n as i64).map(|t| t * 3600).collect()
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

    // ── availability_per_slot (ev-usage-forecast) ───────────────────────────

    #[test]
    fn availability_per_slot_marks_the_trip_window_unavailable() {
        // Weekday leaves 08:00, returns 17:00, jitter 10 min, probability 1.0.
        let cfg = usage_cfg(day_cfg(8, 17, 1.0), day_cfg(8, 17, 1.0));
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 0, 0, 0).unwrap(); // Monday 00:00
        let avail = availability_per_slot(&cfg, 7, now, &hourly_slots(24), 24);
        let trip = daily_trip(&cfg, monday(), 7).unwrap();

        for (t, &ok) in avail.iter().enumerate() {
            let ts = now + Duration::hours(t as i64);
            let inside = ts >= trip.leave_at && ts < trip.return_at;
            assert_eq!(ok, !inside, "slot {t} ({ts}) inside={inside}");
        }
        // Sanity: the window is neither empty nor the whole horizon.
        assert!(avail.iter().any(|&a| a), "some slots must be available");
        assert!(avail.iter().any(|&a| !a), "some slots must be unavailable");
    }

    #[test]
    fn availability_per_slot_all_available_when_no_trip_that_day() {
        let cfg = usage_cfg(day_cfg(8, 17, 0.0), day_cfg(8, 17, 0.0)); // never leaves
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 0, 0, 0).unwrap();
        let avail = availability_per_slot(&cfg, 7, now, &hourly_slots(24), 24);
        assert!(
            avail.iter().all(|&a| a),
            "probability 0.0 must leave every slot available, got {avail:?}"
        );
    }

    #[test]
    fn availability_per_slot_reflects_multiple_trips_in_one_horizon() {
        // 48 h horizon over a daily trip -> two separate away-windows, and the
        // gap between them must be available (the car comes back in between).
        let cfg = usage_cfg(day_cfg(8, 17, 1.0), day_cfg(8, 17, 1.0));
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 0, 0, 0).unwrap();
        let avail = availability_per_slot(&cfg, 7, now, &hourly_slots(48), 48);

        // Count transitions available->unavailable: one per trip start.
        let starts = avail.windows(2).filter(|w| w[0] && !w[1]).count();
        assert_eq!(
            starts, 2,
            "a 48 h horizon over a daily trip must show two departures, got {avail:?}"
        );
    }

    #[test]
    fn availability_per_slot_equals_is_away_at_per_slot() {
        // Guards against a second copy of the prediction logic: the per-slot
        // helper must agree with asking `active_trip_at` directly, slot by slot.
        let cfg = usage_cfg(day_cfg(22, 2, 1.0), day_cfg(22, 2, 1.0)); // midnight-crossing
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 12, 0, 0).unwrap();
        let cum_s = hourly_slots(36);
        let avail = availability_per_slot(&cfg, 7, now, &cum_s, 36);
        for (t, &ok) in avail.iter().enumerate() {
            let ts = now + Duration::seconds(cum_s[t]);
            assert_eq!(
                ok,
                active_trip_at(&cfg, 7, ts).is_none(),
                "slot {t} ({ts}) disagrees with active_trip_at"
            );
        }
    }

    // ── soc_drop_frac_per_slot (ev-usage-forecast) ──────────────────────────

    #[test]
    fn soc_drop_lands_in_the_first_slot_at_or_after_the_return() {
        let cfg = usage_cfg(day_cfg(8, 17, 1.0), day_cfg(8, 17, 1.0));
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 0, 0, 0).unwrap();
        let cum_s = hourly_slots(24);
        let drops = soc_drop_frac_per_slot(&cfg, 7, now, &cum_s, 24);
        let trip = daily_trip(&cfg, monday(), 7).unwrap();

        let nonzero: Vec<usize> = (0..24).filter(|&t| drops[t] > 0.0).collect();
        assert_eq!(nonzero.len(), 1, "exactly one drop per returned trip");
        let t = nonzero[0];
        let slot_start = now + Duration::hours(t as i64);
        let prev_start = now + Duration::hours(t as i64 - 1);
        assert!(
            trip.return_at > prev_start && trip.return_at <= slot_start,
            "drop slot {t} must be the first at-or-after return_at {}",
            trip.return_at
        );
        assert!(
            (drops[t] - trip.soc_drop_pct / 100.0).abs() < 1e-9,
            "drop must equal the trip's own SoC drop"
        );
    }

    #[test]
    fn soc_drop_is_all_zero_when_no_trip_ends_in_the_horizon() {
        let cfg = usage_cfg(day_cfg(8, 17, 0.0), day_cfg(8, 17, 0.0)); // never leaves
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 0, 0, 0).unwrap();
        let drops = soc_drop_frac_per_slot(&cfg, 7, now, &hourly_slots(24), 24);
        assert!(drops.iter().all(|&d| d == 0.0), "got {drops:?}");
    }

    #[test]
    fn soc_drop_never_lands_in_slot_zero() {
        // `now` sits just after a return: the live SoC already reflects that
        // drop, so the plan must not subtract it a second time.
        let cfg = usage_cfg(day_cfg(8, 17, 1.0), day_cfg(8, 17, 1.0));
        let trip = daily_trip(&cfg, monday(), 7).unwrap();
        let now = trip.return_at + Duration::minutes(1);
        let drops = soc_drop_frac_per_slot(&cfg, 7, now, &hourly_slots(24), 24);
        assert_eq!(drops[0], 0.0, "slot 0 must never carry a drop");
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
