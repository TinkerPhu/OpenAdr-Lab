//! Thin tick-path wiring for the site headroom forecast — split out of
//! `helpers.rs` (already near the tasks/ file-size cap) so that file only
//! needs a single delegating line.

use chrono::{DateTime, Duration, Utc};

use crate::controller::capacity_headroom::{
    compute_site_capacity_curve, compute_site_headroom_forecast,
};
use crate::entities::capacity_curve::{CapacityCurve, CommitmentDirection};
use crate::entities::device_session::EvSession;
use crate::entities::plan::{Plan, SiteFlexibilityForecastSlot};
use crate::simulator::SimState;

/// Both forward-looking signals for one tick. Neither needs a separately
/// pre-built PV frame set any more (`pv-competence-consolidation` sections
/// 5a/5b) — PV now flows through the same `Asset::max_effort_setpoint`/
/// `asset_max_power_series`/`simulated_trajectory` primitives every other
/// asset kind uses, since `PvInverter` implements its own weather-aware
/// `max_effort_schedule`/`simulate_forward` — see
/// `controller::capacity_headroom`'s module doc. No active plan → empty
/// forecast (battery/EV/heater/base-load/shiftable-load capacity curves
/// still read the live snapshot directly via `SimState`, unaffected by plan
/// absence).
pub(crate) fn compute_tick_forecasts(
    sim: &SimState,
    plan_snap: Option<&Plan>,
    ev_session: Option<&EvSession>,
    now: DateTime<Utc>,
) -> (
    Vec<SiteFlexibilityForecastSlot>,
    (CapacityCurve, CapacityCurve),
) {
    let forecast = plan_snap
        .map(|plan| compute_site_headroom_forecast(sim, plan, ev_session, now))
        .unwrap_or_default();

    // t2_max sweeps to the plan's own remaining horizon -- falls back to 48h
    // (this profile family's default `plan_horizon_h`, see
    // `entities/planner_params.rs`) when there's no active plan to derive one
    // from.
    let t2_max = plan_snap
        .map(|plan| (plan.horizon.end_time - now).max(Duration::zero()))
        .unwrap_or_else(|| Duration::hours(48));

    let snapshot = sim.to_sim_snapshot();
    let curves = (
        compute_site_capacity_curve(CommitmentDirection::Import, now, t2_max, sim, &snapshot),
        compute_site_capacity_curve(CommitmentDirection::Export, now, t2_max, sim, &snapshot),
    );
    (forecast, curves)
}
