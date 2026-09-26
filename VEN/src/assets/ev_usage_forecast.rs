// ── ev-usage-forecast: the EV's own prediction, handed to the planner ─────────
// Cross-file inherent impl on `EvMilpContext` (whose definition lives in
// `controller::milp_planner::asset_port`), kept out of `ev_milp.rs` so that file
// stays under the 500-line cap. The EV asset is the single authority for "what
// will this car do next" (asset-competence-assurance); this module is where that
// authority reaches the MILP.

use chrono::{DateTime, Duration, Utc};

use super::ev_schedule;
use super::EvCharger;
use crate::controller::milp_planner::asset_port::{EvMilpContext, EvMilpMode, ExogenousSocDrops};
use crate::entities::asset_params::{EvUsageMode, EvUsageSimParams};
use crate::entities::device_session::EvSession;

impl EvMilpContext {
    /// Everything the EV's own predicted usage schedule contributes to this
    /// planning cycle.
    ///
    /// 1. ANDs truthfully-predicted per-slot availability into whatever mask
    ///    `from_state`'s plugged/session logic produced, so the two compose
    ///    rather than one replacing the other. Availability is asserted
    ///    regardless of `engage_charge_planning` and regardless of any real
    ///    session: a session sets a *goal*, it can never make a slot the car is
    ///    predicted to be away for chargeable.
    /// 2. Records the exogenous SoC drops the plan must project but cannot
    ///    decide (what the trip consumes), for the post-solve trajectory.
    /// 3. With `engage_charge_planning`, and only when no real user/VTN session
    ///    already governs the goal, sets the charging target and deadline from
    ///    the next predicted departure — directly here, with no `EvSession`
    ///    written anywhere, so it never competes for the single session slot.
    /// 4. Clamps the core energy to what the now-masked window can physically
    ///    deliver, warning instead of handing the solver an impossible equality.
    ///
    /// A no-op unless the EV declared `usage_forecast` (mode `Forecast`): under
    /// `usage_sim`, or with no usage schedule at all, the context is left
    /// exactly as `from_state` built it. That is what keeps every pre-existing
    /// `EvSession`-deadline behaviour byte-identical.
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
        let available =
            ev_schedule::availability_per_slot(usage, cfg.usage_sim_seed_tag, now, cum_s, n);
        for (slot, ok) in self.a_ev.iter_mut().zip(available) {
            *slot = *slot && ok;
        }
        self.soc_drops = Some(ExogenousSocDrops {
            drop_frac_per_slot: ev_schedule::soc_drop_frac_per_slot(
                usage,
                cfg.usage_sim_seed_tag,
                now,
                cum_s,
                n,
            ),
            floor_frac: usage.min_soc_after_drop_pct / 100.0,
        });

        // A real user/VTN session already said what this EV is charging for;
        // the forecast never overrides a stated goal (availability above still
        // applies either way — that is fact, not preference).
        if usage.engage_charge_planning && ev_session.is_none() {
            self.target_next_predicted_departure(cfg, usage, n, cum_s, now);
        }
        self.clamp_core_to_reachable_energy(n, cum_s);
    }

    /// Sets the charging goal from the next predicted departure inside the plan
    /// horizon: same deadline-step derivation the `EvSession` path uses, so a
    /// predicted departure and a stated one land on the same slot.
    fn target_next_predicted_departure(
        &mut self,
        cfg: &EvCharger,
        usage: &EvUsageSimParams,
        n: usize,
        cum_s: &[i64],
        now: DateTime<Utc>,
    ) {
        if n == 0 {
            return;
        }
        let horizon_end = now + Duration::seconds(cum_s.get(n - 1).copied().unwrap_or(0));
        let Some(trip) =
            ev_schedule::next_trip_after(usage, cfg.usage_sim_seed_tag, now, horizon_end)
        else {
            return;
        };
        let core_kwh = ((cfg.soc_target - self.soc_init) * cfg.battery_kwh).max(0.0);
        if core_kwh <= 1e-6 {
            return; // already at target — nothing to plan for
        }
        let secs = (trip.leave_at - now).num_seconds();
        let t_dead = if secs <= 0 {
            0
        } else {
            cum_s
                .partition_point(|&s| s <= secs)
                .saturating_sub(1)
                .min(n.saturating_sub(1))
        };
        self.mode = EvMilpMode::MustRun;
        self.t_dead_step = Some(t_dead);
        self.e_core_kwh = core_kwh;
    }

    /// Masking slots the car is predicted away for can leave a goal — predicted
    /// or stated by a real session — that no remaining slot can reach. The EV
    /// MILP has no slack on its core-energy equality, so an unreachable core
    /// makes the whole site solve infeasible (see
    /// `tests/solver.rs::solve_ev_must_run_core_energy_beyond_what_the_available_slots_can_deliver`).
    /// Charge as far as the window allows and say so, rather than reject the
    /// request or fail the plan.
    fn clamp_core_to_reachable_energy(&mut self, n: usize, cum_s: &[i64]) {
        if self.mode == EvMilpMode::MustNotRun || self.e_core_kwh <= 1e-6 {
            return;
        }
        let t_dead = self.t_dead_step.unwrap_or(n.saturating_sub(1));
        let reachable_kwh: f64 = (0..n.min(t_dead + 1))
            .filter(|&t| self.a_ev.get(t).copied().unwrap_or(false))
            .map(|t| {
                let dt_s = (cum_s.get(t + 1).copied().unwrap_or(0)
                    - cum_s.get(t).copied().unwrap_or(0))
                .max(0);
                let dt_h = dt_s as f64 / 3600.0;
                self.p_max_kw * dt_h
            })
            .sum();
        if self.e_core_kwh <= reachable_kwh + 1e-6 {
            return;
        }
        // Stable text — WP4.3's notification dedup keys on the message.
        self.core_unmet_warning = Some(format!(
            "EV cannot reach its charging target before departure — the car is \
             predicted away for part of the window; charging {reachable_kwh:.1} kWh \
             of the {:.1} kWh needed",
            self.e_core_kwh
        ));
        self.e_core_kwh = reachable_kwh;
    }
}
