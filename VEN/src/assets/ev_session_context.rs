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
use crate::controller::milp_planner::asset_port::{EvMilpContext, EvMilpMode};

/// WP4.1-c MAX_COST: per-kWh completion reward — an order of magnitude above any
/// real tariff so the solver charges toward the target regardless of price, with
/// the budget constraint (not the price) doing the capping.
const BUDGET_CHARGE_REWARD_EUR_KWH: f64 = 5.0;

impl EvMilpContext {
    /// Construct from a live `AssetState`, sim `EvCharger` config, and optional session data.
    #[allow(clippy::too_many_arguments)]
    pub fn from_state(
        state: &super::AssetState,
        cfg: &EvCharger,
        n: usize,
        cum_s: &[i64],
        now: DateTime<Utc>,
        ev_session: Option<&crate::entities::device_session::EvSession>,
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
            t_dead_step: None,
            p_max_kw: cfg.max_charge_kw,
            p_min_kw: min_charge_kw,
            e_required_kwh: 0.0,
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
        let Some(session) = ev_session else {
            // Plugged, no session: slots available but no charging obligation.
            return Self {
                a_ev: vec![true; n],
                soc_drops: None,
                ..base
            };
        };
        let core_kwh = ((session.target_soc - current_soc) * cfg.battery_kwh).max(0.0);
        let secs = (session.departure_time - now).num_seconds();
        let t_dead = if secs <= 0 {
            0
        } else {
            cum_s
                .partition_point(|&s| s <= secs)
                .saturating_sub(1)
                .min(n.saturating_sub(1))
        };
        let deadline_mask: Vec<bool> = (0..n).map(|t| t <= t_dead).collect();

        match session.mode {
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
                t_dead_step: Some(t_dead),
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
                    t_dead_step: Some(t_dead),
                    // A firm deadline guarantees the target; a soft one expresses
                    // it through the bids instead.
                    e_required_kwh: if session.soft_deadline { 0.0 } else { core_kwh },
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
        }
    }
}
