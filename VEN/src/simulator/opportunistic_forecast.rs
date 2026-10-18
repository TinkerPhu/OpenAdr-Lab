//! The opportunistic objective's forecast (openspec `opportunistic-objective` D6): a clone of
//! the site stepped through the planner's slots with the decision the live tick makes.
//!
//! Per slot it builds the snapshot, feeds the PV and base load the plan forecasts as measured
//! readings (which every asset takes over its own smoothing), reads the requests in force at the
//! slot start, the scheduled import limit and the alert windows, runs
//! `controller::arbiter::opportunistic_step`, the live tick's own decision, and steps
//! `SimState::tick` over the slot's length with `now` at the slot start (so each EV's usage
//! schedule decides when it is plugged, as it does live). One evaluation per slot. Reads only
//! what it is given: it never touches live state, and owns no rule of its own.

use chrono::{DateTime, Utc};
use std::collections::HashMap;

use crate::controller::arbiter::{
    self, limit, DispatchStages, OpportunisticRequests, OverlayEnabled,
};
use crate::controller::forecast_trace::{ForecastGrid, ForecastTrace};
use crate::entities::capacity::{
    tightest_capacity_limit, AlertWindow, CapacitySnapshot, DispatchWindow,
};
use crate::entities::capacity_curve::CommitmentDirection;
use crate::entities::planner_params::PlannerObjective;

use super::tick_inputs::{DEFAULT_BASE_LOAD_ALPHA, DEFAULT_PV_TAU_S};
use super::{AssetTickFeeds, SimState, TickInputs};

/// What the user has asked of the site at one instant, as the live tick reads it.
#[derive(Debug, Clone, Default)]
pub struct SlotRequests {
    pub requests: OpportunisticRequests,
    /// Each EV's opportunistic toggle (its user's switch, paused by its own session).
    pub ev_surplus_switch_on: OverlayEnabled,
    /// Departure of the session each EV is serving.
    pub ev_departure_by_asset: HashMap<String, DateTime<Utc>>,
}

/// What the grid and the user say over the horizon.
pub struct ForecastSite<'a> {
    pub capacity_schedule: &'a [CapacitySnapshot],
    pub dispatch_windows: &'a [DispatchWindow],
    pub alert_windows: &'a [AlertWindow],
    pub limit_enforcement_enabled: bool,
    /// The requests in force at an instant.
    pub requests_at: &'a dyn Fn(DateTime<Utc>) -> SlotRequests,
}

/// Step `sim` through `grid` and record what every asset does.
pub fn opportunistic_forecast(
    mut sim: SimState,
    grid: &ForecastGrid,
    site: &ForecastSite,
) -> ForecastTrace {
    let n = grid.slot_start.len();
    let mut trace = ForecastTrace::default();
    let mut limit_incumbent: Option<String> = None;
    record_states(&sim, &mut trace);
    for t in 0..n {
        let (now, dt_s) = (grid.slot_start[t], grid.slot_s[t]);
        let feeds = AssetTickFeeds {
            measured_kw: grid
                .forecast_kw_by_asset
                .iter()
                .map(|(id, kw)| (id.clone(), kw[t]))
                .collect(),
            ..AssetTickFeeds::default()
        };
        let slot = (site.requests_at)(now);
        let (setpoints, heater_emergency_by_asset, next_incumbent) = slot_decision(
            &sim,
            &feeds,
            &slot,
            site,
            now,
            dt_s,
            limit_incumbent.as_deref(),
        );
        limit_incumbent = next_incumbent;
        sim.tick(TickInputs {
            heater_emergency_by_asset,
            ev_departure_by_asset: slot.ev_departure_by_asset,
            request_targets: slot.requests.targets,
            feeds,
            ..TickInputs::new(dt_s, now, setpoints)
        });
        record_slot(&sim, &mut trace);
        record_states(&sim, &mut trace);
    }
    trace
}

/// One slot's decision: the live tick's opportunistic step on this slot's snapshot. Returns the
/// setpoints, the heaters' emergency modes and the limit pass's lever for the next slot.
#[allow(clippy::type_complexity)]
pub(crate) fn slot_decision(
    sim: &SimState,
    feeds: &AssetTickFeeds,
    slot: &SlotRequests,
    site: &ForecastSite,
    now: DateTime<Utc>,
    dt_s: f64,
    limit_incumbent: Option<&str>,
) -> (
    HashMap<String, f64>,
    HashMap<String, (Option<bool>, Option<bool>)>,
    Option<String>,
) {
    let snap = sim.to_sim_snapshot();
    let live_kw = live_kw(sim, feeds, now, dt_s);
    let thermostat_kw = sim.thermostat_setpoints_kw(None);
    let defaults =
        crate::controller::dispatcher::build_setpoints_without_plan(&snap, &thermostat_kw);
    let alert_active = lab_core::time_window::any_covering(site.alert_windows, now);
    let capacity_limit_kw = tightest_capacity_limit(
        site.capacity_schedule,
        CommitmentDirection::Import,
        now,
        now,
    )
    .map(|l| l.limit_kw);
    let hard_limit_kw = limit::hard_import_limit_kw(capacity_limit_kw, alert_active)
        .filter(|_| site.limit_enforcement_enabled);
    let switch_on = |id: &str| slot.ev_surplus_switch_on.of(id);
    let ev_overlay = arbiter::ev_overlays(&snap, None, now, switch_on);
    let no_bridges = HashMap::new();
    let tick = arbiter::ArbiterTick {
        sim: &snap,
        plan_slot: None,
        objective: PlannerObjective::Opportunistic,
        ev_overlay: &ev_overlay,
        live_kw: &live_kw,
        live_released_kw: Some(&live_kw),
        alert_active,
        limit_target_kw: limit::limit_target_kw(hard_limit_kw, limit_incumbent),
        prev_bridge_kw_by_asset: &no_bridges,
    };
    let stages = DispatchStages {
        dispatch_windows: site.dispatch_windows,
        alert_windows: site.alert_windows,
        comms_loss: None,
        limit_incumbent_lever: limit_incumbent,
    };
    let outcome =
        arbiter::opportunistic_step(&tick, defaults, &slot.requests, switch_on, now, &stages);
    let emergency =
        arbiter::heater_emergency_modes(&Default::default(), &outcome.heater_emergency_mode, &snap);
    let next_incumbent = outcome.limit.as_ref().and_then(|l| l.active_lever.clone());
    (outcome.setpoints, emergency, next_incumbent)
}

/// This slot's PV and base load as the arbiter projects them: each asset's own preview of what
/// the forecast feeds make it do, the same previews the live tick takes.
fn live_kw(
    sim: &SimState,
    feeds: &AssetTickFeeds,
    now: DateTime<Utc>,
    dt_s: f64,
) -> arbiter::LiveKwByAsset {
    sim.iter_assets()
        .filter_map(|(entry, _)| {
            let id = entry.id.as_str();
            let kw = match sim.peek_pv(id, now, dt_s, None, DEFAULT_PV_TAU_S, feeds) {
                Some(preview) => preview.in_force_kw(),
                None => {
                    sim.peek_base_load_kw(id, now, dt_s, None, DEFAULT_BASE_LOAD_ALPHA, feeds)?
                }
            };
            Some((id.to_string(), kw))
        })
        .collect()
}

/// What every asset drew in the slot just stepped, and the site's import and export.
fn record_slot(sim: &SimState, trace: &mut ForecastTrace) {
    let snap = sim.to_sim_snapshot();
    for (id, asset) in &snap.assets {
        trace
            .net_kw_by_asset
            .entry(id.clone())
            .or_default()
            .push(asset.power_kw);
    }
    let net_kw = snap.grid.net_power_kw;
    trace.import_kw.push(net_kw.max(0.0));
    trace.export_kw.push((-net_kw).max(0.0));
}

/// Every asset's state at this step boundary.
fn record_states(sim: &SimState, trace: &mut ForecastTrace) {
    for (id, asset) in &sim.to_sim_snapshot().assets {
        trace
            .states_by_asset
            .entry(id.clone())
            .or_default()
            .push(asset.values.clone());
    }
}

#[cfg(test)]
#[path = "tests/opportunistic_forecast_tests.rs"]
mod tests;
