//! PHASE 5 in-lock tail of the tick loop — split out of `helpers.rs` (over
//! the `tasks/` file-size cap once the capacity-curve forecast was added)
//! so that file only needs a single delegating call, same reasoning as
//! `forecast_wiring.rs`'s own split.

use chrono::{DateTime, Utc};

use crate::controller;
use crate::controller::SimSnapshot;
use crate::entities::capacity_curve::CapacityCurve;
use crate::entities::plan::{SiteFlexibilityEnvelope, SiteFlexibilityForecastSlot};
use crate::simulator::SensorSnapshot;
use crate::simulator::SimState;

/// Extract snapshots, push history, update grid asset, compute envelope +
/// both forward-looking forecasts. Returns the tuple needed for post-lock
/// async state publishing.
///
/// Takes the whole `TickContext` rather than a dozen forwarded fields: every
/// input it needs beyond `sim`/`now` is already resolved there (pre-lock,
/// async), including the per-slot weather PV series and the deterministic PV
/// pin the planner reads as `pv_forecast_override`.
pub(crate) fn finalize_tick_outputs(
    sim: &mut SimState,
    ctx: &super::context::TickContext,
    now: DateTime<Utc>,
) -> (
    SensorSnapshot,
    SimSnapshot,
    SiteFlexibilityEnvelope,
    Vec<SiteFlexibilityForecastSlot>,
    (CapacityCurve, CapacityCurve),
) {
    let tick_sensor = sim.to_sensor_snapshot();
    let tick_sim_snap = sim.to_sim_snapshot();

    // Push HistoryPoint per asset into per-asset ring buffer (CP2).
    {
        use crate::assets::HistoryPoint;
        for entry in &mut sim.assets {
            entry.history.push(HistoryPoint {
                ts: now,
                power_kw: entry.last_power_kw,
                state: entry.state.clone(),
            });
        }
    }

    // Update Grid virtual asset with net power + VTN capacity limits.
    // Done here (not inside tick()) so capacity_snap is available.
    {
        let net_power_kw = sim.grid.net_power_w / 1000.0;
        let import_limit_kw = ctx.capacity_snap.import_limit_kw.unwrap_or(f64::MAX);
        // OadrCapacityState.export_limit_kw is a positive magnitude; negate for sign convention.
        let export_limit_kw_signed = -(ctx.capacity_snap.export_limit_kw.unwrap_or(f64::MAX));
        sim.grid_asset
            .update(net_power_kw, import_limit_kw, export_limit_kw_signed, now);
    }

    // Compute site headroom (pure math — reads the live SimState directly, not
    // the flattened snapshot: needed to call `Asset::max_effort_setpoint`,
    // which the snapshot's `AssetSnapshot`/`capability()` shape can't answer
    // correctly for PV — see `site_headroom.rs`'s module doc).
    let tick_envelope = controller::site_headroom::compute_site_headroom(
        sim,
        now,
        ctx.grid_max_import_kw,
        ctx.grid_max_export_kw,
    );
    let (tick_forecast, tick_capacity_curves) = super::forecast_wiring::compute_tick_forecasts(
        sim,
        ctx.plan_snap.as_ref(),
        now,
        ctx.grid_max_import_kw,
        ctx.grid_max_export_kw,
    );

    (
        tick_sensor,
        tick_sim_snap,
        tick_envelope,
        tick_forecast,
        tick_capacity_curves,
    )
}
