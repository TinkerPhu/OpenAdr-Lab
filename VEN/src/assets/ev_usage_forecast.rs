// ── ev-usage-forecast: the EV's own prediction, handed to the planner ─────────
// Cross-file inherent impl on `EvMilpContext` (whose definition lives in
// `controller::milp_planner::asset_port`), kept out of `ev_milp.rs` so that file
// stays under the 500-line cap. The EV asset is the single authority for "what
// will this car do next" (asset-competence-assurance); this module is where that
// authority reaches the MILP.

use chrono::{DateTime, Duration, Utc};

use super::ev_schedule;
use super::ev_trip_series::{self, ExpectedTripConsumption, ExpectedVehicleUse};
use super::EvCharger;
use crate::controller::milp_planner::asset_port::{EvMilpContext, EvMilpMode, ExogenousSocDrops};
use crate::entities::asset_params::{EvUsageMode, EvUsageSimParams};
use crate::entities::device_session::EvSession;

impl EvMilpContext {
    /// Everything the EV's own predicted usage schedule contributes to this
    /// planning cycle.
    ///
    /// Every predicted trip inside the horizon becomes one expected use, and the
    /// shared derivation turns that series into the same three quantities a stated
    /// series produces: availability, consumption, obligations. Before this, the
    /// forecast stated availability and consumption for every trip but an obligation
    /// for only the *next* departure — so a 48 h horizon holding two trips planned
    /// charging for the first and let the vehicle coast through the second, which is
    /// exactly what ven-12 showed on the live fleet (0.800 → 0.698 → 0.570, flat
    /// between).
    ///
    /// `engage_charge_planning` decides whether those departures are *guarantees*.
    /// With it off, the uses are still stated — the car is still away, the trip still
    /// costs what it costs — but nothing is firm, so no obligation binds and nothing
    /// drives charging. That is one field on the series, not a second code path.
    ///
    /// A no-op unless the EV declared `usage_forecast` (mode `Forecast`): under
    /// `usage_sim`, or with no usage schedule at all, the context is left exactly as
    /// `from_state` built it, which is what keeps every `EvSession`-driven behaviour
    /// byte-identical.
    pub fn apply_usage_forecast(
        &mut self,
        cfg: &EvCharger,
        n: usize,
        cum_s: &[i64],
        now: DateTime<Utc>,
        ev_session: Option<&EvSession>,
    ) {
        let Some(usage) = cfg
            .usage_sim
            .as_ref()
            .filter(|u| u.mode == EvUsageMode::Forecast)
        else {
            return;
        };
        // A real user/VTN session already said what this EV is charging for; the
        // forecast never overrides a stated goal. Availability and consumption below
        // apply either way — those are fact, not preference.
        let firm = usage.engage_charge_planning && ev_session.is_none();
        let uses = predicted_uses(cfg, usage, n, cum_s, now, firm);
        let derived = ev_trip_series::plan_inputs(&uses, n, cum_s, now);

        // ANDed, not replaced: whatever plugged/session logic `from_state` produced
        // still applies, and a slot the car is predicted away for can never become
        // chargeable because a session asked for it.
        for (slot, predicted) in self.a_ev.iter_mut().zip(derived.available_per_slot) {
            *slot = *slot && predicted;
        }
        self.soc_drops = Some(ExogenousSocDrops {
            drop_frac_per_slot: derived.drop_frac_per_slot,
            floor_frac: usage.min_soc_after_drop_pct / 100.0,
        });
        if !derived.obligations.is_empty() {
            self.mode = EvMilpMode::MustRun;
            self.obligations = derived.obligations;
        }
    }
}

/// Every trip the EV predicts inside the horizon, as a series of expected uses.
///
/// The walk is the same one `usage_sim_plan_ahead` performs to fill the session
/// queue — one generator, traversed forward, each window opening where the previous
/// trip returned. That it was already written once for the simulated producer is the
/// evidence that a loop, not a new mechanism, was all the forecast needed (R-92's
/// remaining half).
fn predicted_uses(
    cfg: &EvCharger,
    usage: &EvUsageSimParams,
    n: usize,
    cum_s: &[i64],
    now: DateTime<Utc>,
    firm: bool,
) -> Vec<ExpectedVehicleUse> {
    if n == 0 {
        return Vec::new();
    }
    // cum_s holds n+1 boundaries, so the horizon ENDS at cum_s[n]. Reading cum_s[n-1]
    // instead silently dropped anything in the final slot.
    let horizon_end = now
        + Duration::seconds(
            cum_s
                .get(n)
                .copied()
                .unwrap_or_else(|| cum_s.last().copied().unwrap_or(0)),
        );
    let mut uses = Vec::new();
    let mut cursor = now;
    let mut window_open = now;
    // Mid-trip at `now`. `next_trip_after` only yields trips that depart *after* the
    // cursor, so the trip the car is currently on would be skipped entirely: the plan
    // would mark the rest of the away period chargeable and never subtract what that
    // trip costs. GB-54 makes this the normal case, not an edge one — an aligned plan
    // `now` can sit before wall-clock now, and a replan lands mid-trip routinely.
    //
    // It contributes consumption only, because it has no future departure to be ready
    // for: a zero-width window, which no slot can overlap, and not firm, so it binds
    // nothing. Its drop still lands at its return, and the first chargeable window
    // opens there rather than at `now`.
    if let Some(active) = ev_schedule::active_trip_at(usage, cfg.usage_sim_seed_tag, now) {
        uses.push(ExpectedVehicleUse {
            window_start: now,
            departure_at: now,
            target_soc: cfg.soc_target,
            firm: false,
            consumption: Some(ExpectedTripConsumption {
                return_at: active.return_at,
                soc_drop_frac: active.soc_drop_pct / 100.0,
            }),
            session_id: None,
        });
        window_open = active.return_at;
    }
    while let Some(trip) =
        ev_schedule::next_trip_after(usage, cfg.usage_sim_seed_tag, cursor, horizon_end)
    {
        uses.push(ExpectedVehicleUse {
            window_start: window_open,
            departure_at: trip.leave_at,
            target_soc: cfg.soc_target,
            firm,
            // The generator states the trip's own consumption as a percentage of
            // pack, so no distance is involved and nothing is defaulted: this is a
            // predicted fact, not a user's estimate.
            consumption: Some(ExpectedTripConsumption {
                return_at: trip.return_at,
                soc_drop_frac: trip.soc_drop_pct / 100.0,
            }),
            session_id: None,
        });
        window_open = trip.return_at;
        cursor = trip.leave_at;
    }
    uses
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::asset_params::{EvParams, EvUsageDayParams};
    use chrono::{NaiveDate, NaiveTime, TimeZone};

    // These moved here from `ev_schedule.rs` when `availability_per_slot` and
    // `soc_drop_frac_per_slot` were deleted. The invariants they assert are unchanged
    // and still real — what changed is where the behaviour lives, so the tests follow
    // it rather than being dropped with the functions.

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

    fn forecast_cfg(leave_h: u32, return_h: u32, probability: f64) -> EvCharger {
        let mut ev = EvCharger::from_params(&EvParams {
            battery_kwh: 60.0,
            soc_target: 0.8,
            ..Default::default()
        });
        ev.usage_sim = Some(EvUsageSimParams {
            mode: EvUsageMode::Forecast,
            engage_charge_planning: true,
            weekday: day_cfg(leave_h, return_h, probability),
            weekend: day_cfg(leave_h, return_h, probability),
            min_soc_after_drop_pct: 5.0,
        });
        ev.usage_sim_seed_tag = 7;
        ev
    }

    /// n+1 hourly boundaries — `cum_s[n]` is the horizon end, which is what
    /// `plan_inputs` reads.
    fn hourly_bounds(n: usize) -> Vec<i64> {
        (0..=n as i64).map(|t| t * 3600).collect()
    }

    // 2026-07-20 is a Monday.
    fn monday() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 7, 20).unwrap()
    }

    fn monday_at(h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 20, h, 0, 0).unwrap()
    }

    fn derive(ev: &EvCharger, now: DateTime<Utc>, n: usize) -> ev_trip_series::EvPlanInputs {
        let usage = ev.usage_sim.as_ref().unwrap();
        let cum = hourly_bounds(n);
        let uses = predicted_uses(ev, usage, n, &cum, now, true);
        ev_trip_series::plan_inputs(&uses, n, &cum, now)
    }

    fn trip_of(ev: &EvCharger) -> ev_schedule::UsageTrip {
        ev_schedule::daily_trip(ev.usage_sim.as_ref().unwrap(), monday(), 7).unwrap()
    }

    #[test]
    fn the_trip_window_is_not_chargeable() {
        let ev = forecast_cfg(8, 17, 1.0);
        let now = monday_at(0);
        let out = derive(&ev, now, 24);
        let trip = trip_of(&ev);
        for (t, &ok) in out.available_per_slot.iter().enumerate() {
            let ts = now + Duration::hours(t as i64);
            let inside = ts >= trip.leave_at && ts < trip.return_at;
            assert_eq!(ok, !inside, "slot {t} ({ts}) inside={inside}");
        }
        assert!(out.available_per_slot.iter().any(|&a| a));
        assert!(out.available_per_slot.iter().any(|&a| !a));
    }

    #[test]
    fn a_car_that_never_leaves_is_chargeable_throughout() {
        let ev = forecast_cfg(8, 17, 0.0);
        let out = derive(&ev, monday_at(0), 24);
        assert!(
            out.available_per_slot.iter().all(|&a| a),
            "probability 0.0 leaves every slot available, got {:?}",
            out.available_per_slot
        );
        assert!(
            out.obligations.is_empty(),
            "no trip, so nothing to be ready for"
        );
    }

    #[test]
    fn two_trips_in_one_horizon_each_get_a_window_and_an_obligation() {
        // The defect this change exists to fix: a 48 h horizon over a daily trip shows
        // two departures, and before this BOTH got a mask and a drop while only the
        // FIRST got an obligation - so the plan charged for one and let the car coast
        // through the other.
        let ev = forecast_cfg(8, 17, 1.0);
        let out = derive(&ev, monday_at(0), 48);
        let departures = out
            .available_per_slot
            .windows(2)
            .filter(|w| w[0] && !w[1])
            .count();
        assert_eq!(departures, 2, "{:?}", out.available_per_slot);
        assert_eq!(
            out.obligations.len(),
            2,
            "each departure binds its own target"
        );
        assert!(out.obligations.iter().all(|o| o.target_soc == 0.8));
        assert_eq!(
            out.drop_frac_per_slot.iter().filter(|d| **d > 0.0).count(),
            2,
            "and each return costs the pack once"
        );
    }

    #[test]
    fn availability_agrees_with_active_trip_at_slot_by_slot() {
        // Guards against a second copy of the prediction logic: the derived mask must
        // agree with asking the schedule directly, slot by slot. A midnight-crossing
        // trip, because that is where an independent copy would diverge first.
        let ev = forecast_cfg(22, 2, 1.0);
        let now = monday_at(12);
        let n = 36;
        let out = derive(&ev, now, n);
        let cum = hourly_bounds(n);
        let usage = ev.usage_sim.as_ref().unwrap();
        for (t, &ok) in out.available_per_slot.iter().enumerate() {
            let ts = now + Duration::seconds(cum[t]);
            assert_eq!(
                ok,
                ev_schedule::active_trip_at(usage, 7, ts).is_none(),
                "slot {t} ({ts}) disagrees with active_trip_at"
            );
        }
    }

    #[test]
    fn the_drop_lands_in_the_slot_the_car_returns() {
        let ev = forecast_cfg(8, 17, 1.0);
        let now = monday_at(0);
        let n = 24;
        let out = derive(&ev, now, n);
        let trip = trip_of(&ev);
        let nonzero: Vec<usize> = (0..n)
            .filter(|&t| out.drop_frac_per_slot[t] > 0.0)
            .collect();
        assert_eq!(nonzero.len(), 1, "exactly one drop per returned trip");
        let t = nonzero[0];
        let slot_start = now + Duration::hours(t as i64);
        let prev_start = now + Duration::hours(t as i64 - 1);
        assert!(
            trip.return_at > prev_start && trip.return_at <= slot_start,
            "drop slot {t} must be the first at-or-after return_at {}",
            trip.return_at
        );
        assert!((out.drop_frac_per_slot[t] - trip.soc_drop_pct / 100.0).abs() < 1e-9);
    }

    #[test]
    fn no_drop_when_no_trip_ends_in_the_horizon() {
        let ev = forecast_cfg(8, 17, 0.0);
        let out = derive(&ev, monday_at(0), 24);
        assert!(out.drop_frac_per_slot.iter().all(|&d| d == 0.0));
    }

    #[test]
    fn a_return_just_before_now_is_not_subtracted_again() {
        // The live state of charge already reflects it; counting it again would charge
        // the trip twice.
        let ev = forecast_cfg(8, 17, 1.0);
        let trip = trip_of(&ev);
        let now = trip.return_at + Duration::minutes(1);
        let out = derive(&ev, now, 24);
        assert_eq!(
            out.drop_frac_per_slot[0], 0.0,
            "slot 0 never carries a drop"
        );
    }

    #[test]
    fn a_trip_already_under_way_is_neither_chargeable_nor_free() {
        // `next_trip_after` only yields departures after the cursor, so the trip the
        // car is ON would be skipped: the rest of the away period would read as
        // chargeable and its cost would vanish. GB-54 makes this the normal case, since
        // an aligned plan `now` routinely lands mid-trip.
        let ev = forecast_cfg(8, 17, 1.0);
        let trip = trip_of(&ev);
        let now = trip.leave_at + Duration::hours(1); // out driving
        let n = 24;
        let out = derive(&ev, now, n);
        let cum = hourly_bounds(n);

        for (t, &ok) in out.available_per_slot.iter().enumerate() {
            let ts = now + Duration::seconds(cum[t]);
            if ts < trip.return_at {
                assert!(
                    !ok,
                    "slot {t} ({ts}) is mid-trip and must not be chargeable"
                );
            }
        }
        let total: f64 = out.drop_frac_per_slot.iter().sum();
        assert!(
            (total - trip.soc_drop_pct / 100.0).abs() < 1e-9,
            "the active trip's own drop must still be projected, got {total}"
        );
    }

    #[test]
    fn charge_planning_off_states_the_facts_but_binds_nothing() {
        // Availability and consumption are facts about the car; only the GOAL is
        // conditional on engage_charge_planning. One field on the series, not a
        // second code path.
        let ev = forecast_cfg(8, 17, 1.0);
        let usage = ev.usage_sim.as_ref().unwrap();
        let n = 24;
        let cum = hourly_bounds(n);
        let now = monday_at(0);
        let uses = predicted_uses(&ev, usage, n, &cum, now, false);
        let out = ev_trip_series::plan_inputs(&uses, n, &cum, now);

        assert!(out.obligations.is_empty(), "nothing is promised");
        assert!(
            out.available_per_slot.iter().any(|&a| !a),
            "but the car is still away"
        );
        assert!(
            out.drop_frac_per_slot.iter().any(|&d| d > 0.0),
            "and the trip still costs what it costs"
        );
    }
}
