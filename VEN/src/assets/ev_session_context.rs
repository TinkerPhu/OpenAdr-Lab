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

use super::EvCharger;
use crate::controller::milp_planner::asset_port::{
    EvMilpContext, EvMilpMode, EvObligation, ExogenousSocDrops,
};
use crate::entities::device_session::EvSession;

/// WP4.1-c MAX_COST: per-kWh completion reward — an order of magnitude above any
/// real tariff so the solver charges toward the target regardless of price, with
/// the budget constraint (not the price) doing the capping.
const BUDGET_CHARGE_REWARD_EUR_KWH: f64 = 5.0;


/// The horizon slot an instant falls in, clamped to the horizon's ends.
///
/// The `partition_point` idiom this replaces was written out twice - here and in
/// `ev_usage_forecast::target_next_predicted_departure` - and a deadline landing on
/// the wrong slot is invisible in a plan until something charges a slot too late.
/// One implementation, two callers (`one-concept-one-function`).
pub(super) fn slot_at(cum_s: &[i64], n: usize, secs_from_now: i64) -> usize {
    if secs_from_now <= 0 {
        return 0;
    }
    // Strictly-before, not at-or-before: an instant landing exactly on a slot
    // boundary belongs to the slot that ENDS there, not the one that starts there.
    // With `<=`, a departure at 3600 s on a 300 s grid returned slot 12 - which
    // runs [3600, 3900), entirely after the car has gone. The interior case is
    // unchanged: a departure at 3700 s still returns slot 12, the slot it falls in.
    cum_s
        .partition_point(|&s| s < secs_from_now)
        .saturating_sub(1)
        .min(n.saturating_sub(1))
}

/// Slots in which *some* queued session's charging window is open.
///
/// Generalises the single session's "every slot up to the deadline": with a queue
/// the vehicle is chargeable inside any session's window and nowhere else, so the
/// gaps between sessions - when the car is away - are closed by construction rather
/// than by a separate rule.
fn availability_from_sessions(
    sessions: &[EvSession],
    n: usize,
    cum_s: &[i64],
    now: DateTime<Utc>,
) -> Vec<bool> {
    (0..n)
        .map(|t| {
            let at = now + chrono::Duration::seconds(cum_s.get(t).copied().unwrap_or(0));
            sessions
                .iter()
                .any(|s| s.window_start <= at && at < s.departure_time)
        })
        .collect()
}

/// One obligation per queued session that states a *firm* target and departs inside
/// the horizon.
///
/// A soft deadline states none: under `ev-comfort-piecewise-core` it is a preference
/// priced per kWh by the user's curve, not a guarantee, and the free/opportunistic
/// modes are gated by surplus rather than by a deadline. Sessions departing beyond
/// the horizon contribute nothing to *this* cycle; the next one will see them.
fn obligations_from_sessions(
    sessions: &[EvSession],
    n: usize,
    cum_s: &[i64],
    now: DateTime<Utc>,
) -> Vec<EvObligation> {
    // `cum_s` holds n+1 boundaries: cum_s[t] starts slot t, so the horizon ENDS at
    // cum_s[n], not cum_s[n-1]. Using the latter silently dropped any session
    // departing in the final slot - including the common case of a deadline set
    // exactly at the horizon's end.
    let horizon_end_s = cum_s.get(n).copied().unwrap_or_else(|| {
        cum_s.last().copied().unwrap_or(0)
    });
    sessions
        .iter()
        .filter(|s| !s.soft_deadline && s.mode.states_a_firm_deadline())
        .filter(|s| (s.departure_time - now).num_seconds() <= horizon_end_s)
        .map(|s| EvObligation {
            deadline_step: slot_at(cum_s, n, (s.departure_time - now).num_seconds()),
            target_soc: s.target_soc,
            session_id: Some(s.id),
        })
        .collect()
}


/// The charge each queued session's following trip is expected to consume, placed at
/// the slot the vehicle is next available — i.e. the *next* session's window start.
///
/// This is what makes a manually stated series self-describing. Without it the
/// planner believes the car returns exactly as it left: two stated sessions 48 h
/// apart would see the first charged to target, the trip between them cost nothing,
/// and the second need no charging at all.
///
/// The EV performs every conversion (`asset-competence-assurance`); this only places
/// the results on the grid. `defaulted` rides along so the caller can report that a
/// configured default stood in for a distance the user never gave.
fn trip_drops_between_sessions(
    sessions: &[EvSession],
    cfg: &EvCharger,
    n: usize,
    cum_s: &[i64],
    now: DateTime<Utc>,
) -> (Vec<f64>, bool) {
    let mut drops = vec![0.0; n];
    let mut any_defaulted = false;
    for pair in sessions.windows(2) {
        let (departs, returns) = (&pair[0], &pair[1]);
        let drop = cfg.expected_trip_drop(departs.expected_trip_distance_km);
        if drop.soc_drop_frac <= 0.0 {
            continue;
        }
        any_defaulted |= drop.defaulted;
        // The drop lands when the car is back and chargeable again, which is the next
        // session's window start - the same "first slot at or after the return"
        // convention `ev_schedule::soc_drop_frac_per_slot` already uses.
        let at = slot_at(cum_s, n, (returns.window_start - now).num_seconds());
        // Slot 0 never carries a drop: a return already in the past is reflected in
        // the live state of charge the plan starts from, and counting it again would
        // charge the trip twice.
        if at > 0 {
            drops[at] += drop.soc_drop_frac;
        }
    }
    (drops, any_defaulted)
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
            (s.plugged, s.soc)
        } else {
            (false, 0.0)
        };
        // Idle/unplugged template — every branch below overrides only what differs.
        let base = Self {
            mode: EvMilpMode::MustNotRun,
            soc_init: current_soc,
            a_ev: vec![false; n],
            soc_drops: None,
            obligations: Vec::new(),
            battery_kwh: cfg.battery_kwh,
            p_max_kw: cfg.max_charge_kw,
            p_min_kw: min_charge_kw,
            segments: Vec::new(),
            e_extra_max_kwh: cfg.battery_kwh * (1.0 - cfg.soc_target),
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
                    current_soc,
                    cfg.soc_target,
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
        let core_kwh = ((session.target_soc - current_soc) * cfg.battery_kwh).max(0.0);
        // Chargeable inside any queued session's window, nowhere else. For a single
        // session this is the old "every slot up to the deadline"; for a queue it
        // also closes the gaps when the car is away, without a second rule saying so.
        let deadline_mask = availability_from_sessions(ev_sessions, n, cum_s, now);
        let obligations = obligations_from_sessions(ev_sessions, n, cum_s, now);
        // What the trips between these sessions are expected to cost the pack. Under
        // the forecast usage class `apply_usage_forecast` overwrites this with the
        // EV's own predicted schedule, which is the richer source; this is what makes
        // a *stated* series self-describing when no such schedule exists.
        let (trip_drops, any_defaulted) =
            trip_drops_between_sessions(ev_sessions, cfg, n, cum_s, now);
        let stated_drops = trip_drops.iter().any(|d| *d > 0.0).then(|| ExogenousSocDrops {
            drop_frac_per_slot: trip_drops,
            // Stated sessions declare no floor of their own; the EV's usage config
            // owns that number when it has one.
            floor_frac: cfg
                .usage_sim
                .as_ref()
                .map_or(0.0, |u| u.min_soc_after_drop_pct / 100.0),
        });
        if any_defaulted {
            tracing::debug!(
                "EV session trip consumption defaulted to {} km (no distance stated)",
                cfg.default_trip_distance_km
            );
        }

        let mut ctx = match session.mode {
            // WP4.1 (BL-28) OPPORTUNISTIC / ASAP_FREE: no deadline, no core
            // obligation - all charging is optional "extra" up to the session
            // target, rewarded per charged kWh but gated to free energy via
            // inject_grid_slots. ASAP_FREE additionally biases the reward
            // toward earlier slots.
            UserRequestMode::Opportunistic | UserRequestMode::AsapFree => Self {
                mode: EvMilpMode::MustRun, // core = 0 -> only the gated extra term acts
                a_ev: vec![true; n],
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
                a_ev: vec![true; n],
                soc_drops: None,
                e_extra_max_kwh: core_kwh,
                v_extra_eur_kwh: BUDGET_CHARGE_REWARD_EUR_KWH,
                reward_per_slot: true,
                budget_eur: session.budget_eur,
                ..base
            },
            // WP4.1-c BY_DEADLINE_FREE: the deadline mask stays, but there is
            // no core obligation (free energy may simply not exist) - free-
            // gated per-kWh reward inside the window instead.
            UserRequestMode::ByDeadlineFree => Self {
                mode: EvMilpMode::MustRun,
                a_ev: deadline_mask,
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
                    current_soc,
                    session.target_soc,
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
                    a_ev: deadline_mask,
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
