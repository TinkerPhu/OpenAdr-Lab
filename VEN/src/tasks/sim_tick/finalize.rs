//! PHASE 5 in-lock tail of the tick loop — split out of `helpers.rs` (over
//! the `tasks/` file-size cap once the capacity-curve forecast was added)
//! so that file only needs a single delegating call, same reasoning as
//! `forecast_wiring.rs`'s own split.

use chrono::{DateTime, Utc};

use crate::controller::SimSnapshot;
use crate::entities::capacity::{effective_capacity_limit, CapacitySnapshot};
use crate::entities::capacity_curve::{CapacityCurve, CommitmentDirection};
use crate::entities::plan::{SiteHeadroom, SiteHeadroomForecastSlot};
use crate::entities::sensor::SensorSnapshot;
use crate::entities::sim_inject::SimInjectState;
use crate::simulator;
use crate::simulator::SimState;

/// The grid limits enforced at `now`, as the Grid asset records them: import as a positive
/// magnitude (`f64::MAX` = unlimited), export negated into the site's sign convention
/// (`-f64::MAX` = unlimited). The schedule's VTN limit, else a sim-injected stand-in.
fn enforced_grid_limits_kw(
    capacity_schedule: &[CapacitySnapshot],
    inject: &SimInjectState,
    now: DateTime<Utc>,
) -> (f64, f64) {
    let limit_kw = |direction| {
        effective_capacity_limit(capacity_schedule, inject, direction, now, now)
            .map_or(f64::MAX, |l| l.limit_kw)
    };
    (
        limit_kw(CommitmentDirection::Import),
        -limit_kw(CommitmentDirection::Export),
    )
}

/// Extract snapshots, push history, update grid asset, compute site headroom +
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
    SiteHeadroom,
    Vec<SiteHeadroomForecastSlot>,
    (CapacityCurve, CapacityCurve),
) {
    let tick_sensor = sim.to_sensor_snapshot();
    let tick_sim_snap = sim.to_sim_snapshot();

    // Record the tick: per-asset history points + the Grid virtual asset's net power and VTN
    // capacity limits. Here, not inside tick(), so the capacity schedule is available.
    let (import_limit_kw, export_limit_kw_signed) =
        enforced_grid_limits_kw(&ctx.capacity_schedule, &ctx.inject, now);
    sim.record_history(now, import_limit_kw, export_limit_kw_signed);

    // Compute site headroom (pure math — reads the live SimState directly, not
    // the flattened snapshot: needed to call `Asset::max_effort_setpoint`,
    // which the snapshot's `AssetSnapshot`/`capability()` shape can't answer
    // correctly for PV — see `site_headroom.rs`'s module doc).
    let tick_site_headroom = simulator::site_headroom::compute_site_headroom(
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
        tick_site_headroom,
        tick_forecast,
        tick_capacity_curves,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 15, 12, 0, 0).unwrap()
    }

    fn import_limit(from: DateTime<Utc>, to: DateTime<Utc>, kw: f64) -> CapacitySnapshot {
        CapacitySnapshot {
            interval_start: from,
            interval_end: to,
            import_limit_kw: Some(kw),
            import_limit_event_id: Some("vtn".to_string()),
            ..CapacitySnapshot::default()
        }
    }

    #[test]
    fn the_grid_asset_records_unlimited_when_nothing_applies() {
        let limits = enforced_grid_limits_kw(&[], &SimInjectState::default(), now());
        assert_eq!(limits, (f64::MAX, -f64::MAX));
    }

    #[test]
    fn the_grid_asset_records_the_vtn_limit_in_force_at_now() {
        let schedule = [import_limit(now(), now() + Duration::hours(1), 4.0)];
        let at = |t| enforced_grid_limits_kw(&schedule, &SimInjectState::default(), t).0;
        assert_eq!(at(now() - Duration::seconds(1)), f64::MAX);
        assert_eq!(at(now()), 4.0);
        assert_eq!(at(now() + Duration::hours(1)), f64::MAX);
    }

    /// GB-48 B': the Grid asset used to record the raw VTN fold and so omitted a sim-injected
    /// limit the arbiter was enforcing; it records what is enforced.
    #[test]
    fn the_grid_asset_records_an_injected_limit_where_no_vtn_limit_applies() {
        let inject = SimInjectState {
            grid_import_limit_kw: Some(7.0),
            grid_export_limit_kw: Some(3.0),
            ..SimInjectState::default()
        };
        assert_eq!(enforced_grid_limits_kw(&[], &inject, now()), (7.0, -3.0));
        let schedule = [import_limit(now(), now() + Duration::hours(1), 4.0)];
        assert_eq!(
            enforced_grid_limits_kw(&schedule, &inject, now()),
            (4.0, -3.0),
            "a VTN limit is never overridden"
        );
    }
}
