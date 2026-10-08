// ── A session becomes a MILP context ─────────────────────────────────────────
// `EvMilpContext::from_state` — how the EV's live state and the user's request
// turn into the quantities the planner optimises. Split out of `ev_milp.rs` to
// stay under the VEN/src/ 500-production-line cap (`ven-architecture` rule);
// a cross-file inherent impl, the same pattern `ev_usage_forecast.rs` uses.
//
// One arm per `UserRequestMode`. Only `ByDeadline`/`Asap` read the comfort curve
// (`ev-comfort-piecewise-core`); the free/opportunistic modes are gated by PV
// surplus or a budget and reward each charged kWh at a flat rate instead.

use chrono::{DateTime, Utc};

use super::ev_trip_series;
use super::EvCharger;
use crate::controller::milp_planner::asset_port::{EvMilpContext, EvMilpMode, ExogenousSocDrops};
use crate::entities::device_session::EvSession;

/// WP4.1-c MAX_COST: per-kWh completion reward — an order of magnitude above any
/// real tariff so the solver charges toward the target regardless of price, with
/// the budget constraint (not the price) doing the capping.
const BUDGET_CHARGE_REWARD_EUR_KWH: f64 = 5.0;

/// One queued session states one expected use of the vehicle.
///
/// This is the whole of the session producer's job: say what is expected, and let
/// `ev_trip_series` decide what that means for the plan. The consumption is `Some`
/// only when the user stated BOTH a distance and a return time — the pair is what
/// makes a drop placeable, and the route boundary refuses a half-stated one, so
/// reaching here with one of the two is already impossible.
///
/// `horizon_end` is what "no departure known" is stated as, the same way the usage
/// forecast states a vehicle that stays home: a mode with no deadline charges whenever it
/// can, so its `departure_time` is not a time the car leaves. It becomes one only when the
/// user also stated the trip — then the car really is away from the departure to the
/// return, in every mode, and the series says so.
fn uses_from_sessions(
    sessions: &[EvSession],
    cfg: &EvCharger,
    horizon_end: DateTime<Utc>,
) -> Vec<ev_trip_series::ExpectedVehicleUse> {
    sessions
        .iter()
        .map(|s| ev_trip_series::ExpectedVehicleUse {
            window_start: s.window_start,
            departure_at: if s.mode.charges_until_departure() || s.expected_return_time.is_some() {
                s.departure_time
            } else {
                horizon_end
            },
            target_soc_frac: s.target_soc_frac,
            // A soft deadline is a preference priced by the curve, and the
            // free/opportunistic modes are gated by surplus rather than a deadline:
            // neither states a guarantee.
            firm: !s.soft_deadline && s.mode.states_a_firm_deadline(),
            consumption: match (s.expected_trip_distance_km, s.expected_return_time) {
                (Some(km), Some(return_at)) => {
                    Some(ev_trip_series::ExpectedTripConsumption {
                        return_at,
                        // The EV performs every conversion
                        // (`asset-competence-assurance`); this only carries the result.
                        soc_drop_frac: cfg.expected_trip_drop_frac(km),
                    })
                }
                _ => None,
            },
            session_id: Some(s.id),
        })
        .collect()
}

impl EvMilpContext {
    /// Construct from a live `AssetState`, sim `EvCharger` config, and optional session data.
    #[allow(clippy::too_many_arguments)]
    pub fn from_state(
        state: &super::AssetState,
        cfg: &EvCharger,
        n: usize,
        cum_s: &[i64],
        now: DateTime<Utc>,
        ev_sessions: &[EvSession],
        comfort_rates: &[crate::entities::asset::ComfortRate],
        min_charge_kw: f64,
        v_ev_extra_eur_kwh: f64,
        v_ev_core_eur_kwh: f64,
        asap_lateness_eur_kwh_h: f64,
        v_ev_free_charge_eur_kwh: f64,
        w_ghg_eur_kg: f64,
    ) -> Self {
        use crate::entities::design_vocabulary::UserRequestMode;
        let (plugged, current_soc) = if let super::AssetState::Ev(s) = state {
            (s.plugged, s.soc_frac)
        } else {
            (false, 0.0)
        };
        // Idle/unplugged template — every branch below overrides only what differs.
        let base = Self {
            mode: EvMilpMode::MustNotRun,
            soc_init_frac: current_soc,
            // One declared ceiling for the whole context, so no arm below can plan past
            // what `capability_inner` will accept.
            soc_max_frac: cfg.soc_target_frac,
            a_ev: vec![false; n],
            soc_drops: None,
            obligations: Vec::new(),
            battery_kwh: cfg.battery_kwh,
            p_max_kw: cfg.max_charge_kw,
            p_min_kw: min_charge_kw,
            segments: Vec::new(),
            e_extra_max_kwh: cfg.battery_kwh * (1.0 - cfg.soc_target_frac),
            v_extra_eur_kwh: v_ev_extra_eur_kwh,
            asap_lateness_eur_kwh_h: 0.0,
            free_only: false,
            p_free_cap_kw: None,
            reward_per_slot: false,
            free_early_bias: false,
            budget_eur: None,
            c_imp_eur_kwh: None,
            v_extra_co2_eur_kwh: 0.0,
        };
        // `ev-usage-forecast`: presence for future slots comes from the EV's own
        // schedule, not the live plug — blanking the horizon here left a mid-trip
        // VEN with no charging plan at all. `apply_usage_forecast` then ANDs the
        // prediction in, which is false for every away slot, slot 0 included.
        let forecast_presence = cfg
            .usage_sim
            .as_ref()
            .is_some_and(|u| u.mode == crate::entities::asset_params::EvUsageMode::Forecast);
        if !plugged && !forecast_presence {
            return base;
        }
        // The head session keeps today's role: it supplies the mode, the comfort
        // curve and the valuation. Sessions behind it contribute obligations only -
        // their bands would have to be priced from a starting SoC the solver has not
        // decided yet (design Decision 5).
        let Some(session) = ev_sessions.first() else {
            // Plugged, no session: slots available but no charging obligation.
            //
            // The bands are still built, because they do not depend on a deadline
            // — they are a function of the curve, the current SoC, the target and
            // the pack size, all known here. `apply_usage_forecast` may then turn
            // this into a real obligation from a *predicted* departure
            // (`engage_charge_planning`), and when it does the energy must be
            // priced by the user's curve exactly as a requested charge is. Leaving
            // `segments` empty here is what made the fleet's own charge planning
            // bypass the curve entirely and fall back to the flat `e_ev_extra`
            // reward, i.e. the model `ev-comfort-piecewise-core` replaced.
            //
            // Under `MustNotRun` (nothing has made the EV runnable) `declare_vars`
            // emits no band variables at all, so carrying them costs nothing.
            return Self {
                a_ev: vec![true; n],
                soc_drops: None,
                segments: super::ev_comfort::ev_energy_segments(
                    comfort_rates,
                    super::ev_comfort::EvBandRange {
                        init: current_soc,
                        target: cfg.soc_target_frac,
                        max: cfg.soc_target_frac,
                    },
                    cfg.battery_kwh,
                    v_ev_core_eur_kwh,
                    v_ev_extra_eur_kwh,
                    w_ghg_eur_kg,
                ),
                // Priced per band, so nothing may also be bought through the flat
                // beyond-target reward — same reason as the `ByDeadline` arm.
                e_extra_max_kwh: 0.0,
                v_extra_eur_kwh: 0.0,
                ..base
            };
        };
        let core_kwh = ((session.target_soc_frac - current_soc) * cfg.battery_kwh).max(0.0);
        // Chargeable inside any queued session's window, nowhere else. For a single
        // session this is the old "every slot up to the deadline"; for a queue it
        // also closes the gaps when the car is away, without a second rule saying so.
        // The stated series, through the one derivation that serves both producers.
        // Availability, consumption and obligations all come from the same walk, so a
        // session cannot be chargeable by one rule and obliged by another — in any
        // mode: every arm below takes this mask, none builds its own.
        let horizon_end =
            now + chrono::Duration::seconds(cum_s.get(n).or(cum_s.last()).copied().unwrap_or(0));
        let uses = uses_from_sessions(ev_sessions, cfg, horizon_end);
        let derived = ev_trip_series::plan_inputs(&uses, n, cum_s, now);
        let available_per_slot = derived.available_per_slot;
        let obligations = derived.obligations;
        let stated_drops = derived
            .drop_frac_per_slot
            .iter()
            .any(|d| *d > 0.0)
            .then(|| ExogenousSocDrops {
                drop_frac_per_slot: derived.drop_frac_per_slot,
                // Stated sessions declare no floor of their own; the EV's usage config
                // owns that number when it has one.
                floor_frac: cfg
                    .usage_sim
                    .as_ref()
                    .map_or(0.0, |u| u.min_soc_after_drop_pct / 100.0),
            });

        let mut ctx = match session.mode {
            // WP4.1 (BL-28) OPPORTUNISTIC / ASAP_FREE: no deadline, no core
            // obligation - all charging is optional "extra" up to the session
            // target, rewarded per charged kWh but gated to free energy via
            // inject_grid_slots. ASAP_FREE additionally biases the reward
            // toward earlier slots.
            UserRequestMode::Opportunistic | UserRequestMode::AsapFree => Self {
                mode: EvMilpMode::MustRun, // core = 0 -> only the gated extra term acts
                a_ev: available_per_slot,
                soc_drops: None,
                e_extra_max_kwh: core_kwh,
                v_extra_eur_kwh: v_ev_free_charge_eur_kwh,
                free_only: true,
                reward_per_slot: true,
                free_early_bias: session.mode == UserRequestMode::AsapFree,
                ..base
            },
            // WP4.1-c MAX_COST: complete whenever, but total charging cost
            // stays within the budget (hard constraint from the injected
            // import rates). Completion is a per-kWh reward high enough to
            // beat any real tariff, NOT a hard core constraint - an
            // unaffordable target degrades to partial charging + a plan
            // warning, never an infeasible solve.
            UserRequestMode::MaxCost => Self {
                mode: EvMilpMode::MustRun,
                a_ev: available_per_slot,
                soc_drops: None,
                e_extra_max_kwh: core_kwh,
                v_extra_eur_kwh: BUDGET_CHARGE_REWARD_EUR_KWH,
                reward_per_slot: true,
                budget_eur: session.budget_eur,
                ..base
            },
            // WP4.1-c BY_DEADLINE_FREE: the window closes at the deadline, but there is
            // no core obligation (free energy may simply not exist) - free-
            // gated per-kWh reward inside the window instead.
            UserRequestMode::ByDeadlineFree => Self {
                mode: EvMilpMode::MustRun,
                a_ev: available_per_slot,
                soc_drops: None,
                e_extra_max_kwh: core_kwh,
                v_extra_eur_kwh: v_ev_free_charge_eur_kwh,
                free_only: true,
                reward_per_slot: true,
                ..base
            },
            // Legacy BY_DEADLINE (+ ASAP, which only adds the lateness
            // penalty): hard/soft core energy by the departure deadline.
            // BL-34: this is the only arm that reads the session's comfort curve.
            // Every other arm above redirects `v_extra_eur_kwh` to an unrelated
            // signal (free-energy incentive, budget reward), so the curve does
            // not apply there.
            UserRequestMode::ByDeadline | UserRequestMode::Asap => {
                // The only arm that reads the comfort curve: every kWh from here
                // to full is priced by the user's own bid at that state of charge,
                // so a bid covering part of the energy buys that part (GB-41).
                let segments = super::ev_comfort::ev_energy_segments(
                    &session.comfort_rates,
                    super::ev_comfort::EvBandRange {
                        init: current_soc,
                        target: session.target_soc_frac,
                        // The vehicle's limit, not the request's: a session asking for
                        // more than the charger accepts does not make it accept it. The
                        // gap surfaces as a reported shortfall, not as energy to buy.
                        max: cfg.soc_target_frac,
                    },
                    cfg.battery_kwh,
                    v_ev_core_eur_kwh,
                    v_ev_extra_eur_kwh,
                    w_ghg_eur_kg,
                );
                Self {
                    mode: if session.soft_deadline {
                        EvMilpMode::MayRun
                    } else {
                        EvMilpMode::MustRun
                    },
                    a_ev: available_per_slot,
                    soc_drops: None,
                    segments,
                    // Inert here: this arm prices per band, so nothing may also
                    // be bought through `e_ev_extra`.
                    e_extra_max_kwh: 0.0,
                    v_extra_eur_kwh: 0.0,
                    v_extra_co2_eur_kwh: 0.0,
                    asap_lateness_eur_kwh_h: if session.mode == UserRequestMode::Asap {
                        asap_lateness_eur_kwh_h
                    } else {
                        0.0
                    },
                    ..base
                }
            }
        };
        // Set once, for every arm: the list is already filtered per session, so an
        // opportunistic or budget-capped *head* contributes none of its own while a
        // firm session queued behind it still does. Setting this per arm silently
        // dropped exactly that case.
        ctx.obligations = obligations;
        // Same reasoning as the obligations above: set once for every arm rather than
        // per arm, so no mode can silently lose the drops.
        if ctx.soc_drops.is_none() {
            ctx.soc_drops = stated_drops;
        }
        ctx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::device_session::EvSessionOrigin;
    use chrono::{Duration, TimeZone};

    /// Built from the real asset rather than hand-assembled
    /// (`asset-competence-assurance`): these tests only need it to satisfy the
    /// mapper's signature, but a fixture that invents an asset's config is the same
    /// violation as code that does.
    fn ev_cfg() -> EvCharger {
        EvCharger::from_params(&crate::entities::asset_params::EvParams::default())
    }

    fn sess(window_start: DateTime<Utc>, departure: DateTime<Utc>) -> EvSession {
        EvSession {
            id: uuid::Uuid::new_v4(),
            target_soc_frac: 0.9,
            window_start,
            departure_time: departure,
            expected_trip_distance_km: None,
            expected_return_time: None,
            soft_deadline: false,
            origin: EvSessionOrigin::UserRequest,
            mode: Default::default(),
            budget_eur: None,
            comfort_rates: vec![],
            created_at: window_start,
            updated_at: window_start,
        }
    }

    /// The regression this pins cost up to an hour of charging per planned session on
    /// the live fleet, and it is the third bug of the same family: GB-54 aligns a
    /// plan's `now` to the slot grid, so a session created at 15:25 belongs to a slot
    /// that began at 15:00. Asking "does the window contain the slot's *start*" locked
    /// the EV out of the whole slot in progress — the one slot dispatch actually acts
    /// on. A slot is chargeable when the window *overlaps* it.
    #[test]
    fn availability_includes_the_slot_a_window_opens_partway_through() {
        let now = Utc.with_ymd_and_hms(2026, 10, 3, 15, 0, 0).unwrap();
        let n = 4;
        let cum_s: Vec<i64> = (0..=n as i64).map(|t| t * 3600).collect();
        // Created 25 minutes into slot 0, as a user request mid-slot is.
        let s = sess(now + Duration::minutes(25), now + Duration::hours(3));

        let uses = uses_from_sessions(
            std::slice::from_ref(&s),
            &ev_cfg(),
            now + Duration::hours(n as i64),
        );
        let mask = ev_trip_series::plan_inputs(&uses, n, &cum_s, now).available_per_slot;

        assert!(
            mask[0],
            "the slot the window opens inside must be chargeable"
        );
        assert!(mask[1] && mask[2], "and the slots fully inside it");
    }

    #[test]
    fn availability_excludes_slots_outside_every_window() {
        let now = Utc.with_ymd_and_hms(2026, 10, 3, 15, 0, 0).unwrap();
        let n = 6;
        let cum_s: Vec<i64> = (0..=n as i64).map(|t| t * 3600).collect();
        // Away until 17:00, back for 17:00-19:00 only.
        let s = sess(now + Duration::hours(2), now + Duration::hours(4));

        let uses = uses_from_sessions(
            std::slice::from_ref(&s),
            &ev_cfg(),
            now + Duration::hours(n as i64),
        );
        let mask = ev_trip_series::plan_inputs(&uses, n, &cum_s, now).available_per_slot;

        assert_eq!(
            mask,
            vec![false, false, true, true, false, false],
            "chargeable only where the window overlaps"
        );
    }

    fn plugged_state() -> super::super::AssetState {
        super::super::AssetState::Ev(super::super::EvState {
            soc_frac: 0.30,
            plugged: true,
            actual_power_kw: 0.0,
            pending_command_kw: 0.0,
            was_away_by_usage_sim: false,
        })
    }

    fn a_ev_for(session: &EvSession, n: usize, now: DateTime<Utc>) -> Vec<bool> {
        let cum_s: Vec<i64> = (0..=n as i64).map(|t| t * 3600).collect();
        EvMilpContext::from_state(
            &plugged_state(),
            &ev_cfg(),
            n,
            &cum_s,
            now,
            std::slice::from_ref(session),
            &[],
            0.0,
            1.0,
            1.0,
            0.0,
            0.0,
            0.0,
        )
        .a_ev
    }

    /// A mode with no deadline does not make the vehicle present while it is driving.
    /// The mask and the trip's charge loss used to come from different rules in these
    /// modes: the drop was booked at the return, yet every slot of the trip stayed
    /// chargeable — a plan that charges a car it has just said is away.
    #[test]
    fn from_state_a_stated_trip_closes_availability_in_every_mode() {
        use crate::entities::design_vocabulary::UserRequestMode;
        let now = Utc.with_ymd_and_hms(2026, 10, 3, 15, 0, 0).unwrap();
        for mode in [
            UserRequestMode::Opportunistic,
            UserRequestMode::AsapFree,
            UserRequestMode::MaxCost,
            UserRequestMode::ByDeadline,
            UserRequestMode::ByDeadlineFree,
        ] {
            // Leaves at 17:00, back at 19:00.
            let mut s = sess(now, now + Duration::hours(2));
            s.mode = mode.clone();
            s.expected_trip_distance_km = Some(40.0);
            s.expected_return_time = Some(now + Duration::hours(4));

            assert_eq!(
                a_ev_for(&s, 6, now),
                vec![true, true, false, false, true, true],
                "{mode:?}: chargeable before the trip and after the return, not during it"
            );
        }
    }

    /// Without a stated trip, a no-deadline mode's `departure_time` is a deadline it
    /// does not have, not a statement that the car leaves: the window stays open.
    #[test]
    fn from_state_a_bare_departure_closes_availability_only_in_deadline_modes() {
        use crate::entities::design_vocabulary::UserRequestMode;
        let now = Utc.with_ymd_and_hms(2026, 10, 3, 15, 0, 0).unwrap();
        for (mode, closes) in [
            (UserRequestMode::Opportunistic, false),
            (UserRequestMode::AsapFree, false),
            (UserRequestMode::MaxCost, false),
            (UserRequestMode::ByDeadline, true),
            (UserRequestMode::Asap, true),
            (UserRequestMode::ByDeadlineFree, true),
        ] {
            let mut s = sess(now, now + Duration::hours(2));
            s.mode = mode.clone();
            let expected = if closes {
                vec![true, true, false, false]
            } else {
                vec![true; 4]
            };
            assert_eq!(a_ev_for(&s, 4, now), expected, "{mode:?}");
        }
    }
}
