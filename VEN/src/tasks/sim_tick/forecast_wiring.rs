//! Thin tick-path wiring for the site headroom forecast — split out of
//! `helpers.rs` (already near the tasks/ file-size cap) so that file only
//! needs a single delegating line.

use chrono::{DateTime, Duration, Utc};

use crate::controller::capacity_envelope::{
    compute_site_capacity_curve, compute_site_headroom_forecast,
};
use crate::entities::capacity_curve::{CapacityCurve, CommitmentDirection};
use crate::entities::device_session::EvSession;
use crate::entities::plan::{Plan, SiteFlexibilityForecastSlot};
use crate::simulator::forecast::build_forecast_frames;
use crate::simulator::SimState;

/// Both forward-looking signals for one tick, sharing a single
/// `build_forecast_frames` call for PV's weather-driven frames (the one
/// input both `compute_site_headroom_forecast` and `compute_site_capacity_curve`
/// still need from it — see `controller::capacity_envelope`'s module doc for
/// why PV stays on this path while every other asset kind goes through the
/// shared `Asset::max_effort_setpoint`/`asset_max_power_series` primitives
/// instead). No active plan → empty forecast and PV-frame-less capacity
/// curves (battery/EV/heater/base-load/shiftable-load still read the live
/// snapshot directly via `SimState`, unaffected by plan absence).
pub(crate) fn compute_tick_forecasts(
    sim: &SimState,
    plan_snap: Option<&Plan>,
    ev_session: Option<&EvSession>,
    weather_pv_kw_slots: Option<&[f64]>,
    pv_forecast_override: Option<f64>,
    now: DateTime<Utc>,
) -> (
    Vec<SiteFlexibilityForecastSlot>,
    (CapacityCurve, CapacityCurve),
) {
    let frames = plan_snap
        .map(|plan| {
            build_forecast_frames(
                sim,
                plan,
                ev_session,
                weather_pv_kw_slots,
                pv_forecast_override,
                now,
            )
        })
        .unwrap_or_default();

    let forecast = plan_snap
        .map(|plan| compute_site_headroom_forecast(sim, plan, ev_session, &frames, now))
        .unwrap_or_default();

    // t2_max sweeps to the plan's own remaining horizon (matching `frames`'
    // own range, so PV's Export contribution stays consistent between the
    // two) -- falls back to 48h (this profile family's default
    // `plan_horizon_h`, see `entities/planner_params.rs`) when there's no
    // active plan to derive one from.
    let t2_max = plan_snap
        .map(|plan| (plan.horizon.end_time - now).max(Duration::zero()))
        .unwrap_or_else(|| Duration::hours(48));

    let snapshot = sim.to_sim_snapshot();
    let curves = (
        compute_site_capacity_curve(
            CommitmentDirection::Import,
            now,
            t2_max,
            sim,
            &frames,
            &snapshot,
        ),
        compute_site_capacity_curve(
            CommitmentDirection::Export,
            now,
            t2_max,
            sim,
            &frames,
            &snapshot,
        ),
    );
    (forecast, curves)
}
