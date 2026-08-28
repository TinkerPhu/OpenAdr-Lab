//! Thin tick-path wiring for the site headroom forecast — split out of
//! `helpers.rs` (already near the tasks/ file-size cap) so that file only
//! needs a single delegating line.

use chrono::{DateTime, Utc};

use crate::controller::capacity_forecast::compute_capacity_curve;
use crate::controller::envelope_forecast::compute_headroom_forecast;
use crate::entities::capacity_curve::{CapacityCurve, CommitmentDirection};
use crate::entities::device_session::{EvSession, ShiftableLoad, ShiftableLoadRuntime};
use crate::entities::plan::{Plan, SiteFlexibilityForecastSlot};
use crate::simulator::forecast::build_forecast_frames;
use crate::simulator::SimState;

/// Both forward-looking signals for one tick, sharing a single
/// `build_forecast_frames` call: the per-slot headroom forecast (plan-driven,
/// independent-counterfactual — `envelope_forecast::compute_headroom_forecast`)
/// and the sustained-commitment capacity curves (import, export — closed-form,
/// `capacity_forecast::compute_capacity_curve`, deliberately NOT plan-driven
/// for battery/EV/heater/base load; only PV's forecast-frame data is reused,
/// since that's weather-driven, not plan-driven — see that module's doc
/// comment). No active plan → empty forecast, and capacity curves with no PV
/// contribution (battery/EV/heater/base load still read the live snapshot
/// directly, unaffected by plan absence).
#[allow(clippy::too_many_arguments)]
pub(crate) fn compute_tick_forecasts(
    sim: &SimState,
    plan_snap: Option<&Plan>,
    ev_session: Option<&EvSession>,
    shiftable_loads: &[ShiftableLoad],
    shiftable_runtimes: &[ShiftableLoadRuntime],
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
        .map(|plan| compute_headroom_forecast(&frames, plan, shiftable_loads, shiftable_runtimes))
        .unwrap_or_default();

    let snapshot = sim.to_sim_snapshot();
    let curves = (
        compute_capacity_curve(
            CommitmentDirection::Import,
            now,
            &snapshot,
            &frames,
            shiftable_loads,
            shiftable_runtimes,
        ),
        compute_capacity_curve(
            CommitmentDirection::Export,
            now,
            &snapshot,
            &frames,
            shiftable_loads,
            shiftable_runtimes,
        ),
    );
    (forecast, curves)
}
