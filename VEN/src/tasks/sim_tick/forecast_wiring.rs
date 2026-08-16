//! Thin tick-path wiring for the site headroom forecast — split out of
//! `helpers.rs` (already near the tasks/ file-size cap) so that file only
//! needs a single delegating line.

use chrono::{DateTime, Utc};

use crate::controller::envelope_forecast::compute_headroom_forecast;
use crate::entities::device_session::{EvSession, ShiftableLoad, ShiftableLoadRuntime};
use crate::entities::plan::{Plan, SiteFlexibilityForecastSlot};
use crate::simulator::forecast::build_forecast_frames;
use crate::simulator::SimState;

/// No active plan → no forecast (matches the live envelope's own "always
/// queryable, degrades to nothing meaningful without a plan" shape).
pub(crate) fn compute_tick_forecast(
    sim: &SimState,
    plan_snap: Option<&Plan>,
    ev_session: Option<&EvSession>,
    shiftable_loads: &[ShiftableLoad],
    shiftable_runtimes: &[ShiftableLoadRuntime],
    now: DateTime<Utc>,
) -> Vec<SiteFlexibilityForecastSlot> {
    let Some(plan) = plan_snap else {
        return Vec::new();
    };
    let frames = build_forecast_frames(sim, plan, ev_session, now);
    compute_headroom_forecast(&frames, plan, shiftable_loads, shiftable_runtimes)
}
