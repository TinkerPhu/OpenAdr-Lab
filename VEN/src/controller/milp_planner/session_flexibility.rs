//! The plan's session flexibility: for each session an asset is serving, how much energy it
//! needs, in which window, at which power, and what it is estimated to cost. One rule over the
//! window each asset declares (`AssetMilpContext::flexibility_window`); this module knows no
//! asset kind and never decides what a session needs.

use crate::entities::units::dt_h_from_duration;
use chrono::{DateTime, Utc};

use crate::controller::milp_planner::{AssetMilpContext, FlexibilityWindow};
use crate::entities::plan::{PlanTimeSlot, SessionFlexibility};
use crate::entities::planner_params::PlannerParams;

use super::types::*;

/// Session cost/CO2 from the solved schedule: for every allocation of the asset
/// in slots before `window_end`, the grid share is priced at the slot import
/// tariff and the PV-surplus share at its export opportunity price (the revenue
/// forgone by not exporting it). CO2 comes from the allocation (grid share only
/// — surplus is zero-carbon). `None` when the schedule has no allocation for
/// the asset, so callers can fall back to a pre-solve estimate.
fn solved_session_cost(
    slots: &[PlanTimeSlot],
    asset_id: &str,
    window_end: DateTime<Utc>,
) -> Option<(f64, f64)> {
    let mut found = false;
    let mut cost_eur = 0.0;
    let mut co2_g = 0.0;
    for slot in slots.iter().filter(|s| s.start < window_end) {
        let dt_h = dt_h_from_duration(slot.end - slot.start);
        for alloc in slot.allocations.iter().filter(|a| a.asset_id == asset_id) {
            found = true;
            cost_eur += (alloc.grid_power_kw * slot.import_tariff_eur_kwh
                + alloc.surplus_power_kw * slot.export_tariff_eur_kwh)
                * dt_h;
            co2_g += alloc.co2_g;
        }
    }
    found.then_some((cost_eur, co2_g))
}

/// One session's flexibility entry by the one rule: the window runs from the later of its
/// declared start and `now` to its declared end; the cost and CO2 come from the solved schedule
/// when there is one, else from the declared energy at the window's average import tariff and
/// CO2 intensity.
fn session_entry(
    asset_id: &str,
    window: FlexibilityWindow,
    inputs: &MilpInputs,
    step_s: i64,
    now: DateTime<Utc>,
    solved_slots: Option<&[PlanTimeSlot]>,
) -> SessionFlexibility {
    let n = inputs.n;
    let window_start = window.window_start.max(now);
    let window_end = window.window_end;
    let slots_available = ((window_end - window_start).num_seconds() / step_s).max(0) as usize;
    let t_start = ((window_start - now).num_seconds() / step_s).max(0) as usize;
    let t_end = ((window_end - now).num_seconds() / step_s)
        .max(0)
        .min(n as i64) as usize;
    let count = (t_start..t_end).len().max(1) as f64;
    let avg_tariff = (t_start..t_end)
        .map(|t| inputs.c_imp_eur_kwh[t])
        .sum::<f64>()
        / count;
    let avg_co2 = (t_start..t_end)
        .map(|t| inputs.g_imp_kgco2_kwh[t] * 1000.0)
        .sum::<f64>()
        / count;
    let energy_needed_kwh = window.energy_needed_kwh;
    let (estimated_cost_eur, estimated_co2_g) = solved_slots
        .and_then(|s| solved_session_cost(s, asset_id, window_end))
        .unwrap_or((energy_needed_kwh * avg_tariff, energy_needed_kwh * avg_co2));
    SessionFlexibility {
        asset_id: asset_id.to_string(),
        energy_needed_kwh,
        power_min_kw: window.power_min_kw,
        power_max_kw: window.power_max_kw,
        window_start,
        window_end,
        slots_available,
        max_acceptable_rate_eur_kwh: 0.35,
        min_acceptable_rate_eur_kwh: 0.05,
        budget_remaining_eur: 1.0e9,
        estimated_cost_eur,
        estimated_co2_g,
    }
}

/// Session flexibility for every asset that declares a session it is serving, in planner input
/// order.
///
/// `solved_slots` is the translated schedule when a solution exists; estimates
/// are then derived from the actual allocations. Without it (fallback plan),
/// estimates degrade to energy × average import tariff over the window.
pub(crate) fn build_session_flexibility(
    asset_contexts: &[Box<dyn AssetMilpContext>],
    inputs: &MilpInputs,
    planner: &PlannerParams,
    now: DateTime<Utc>,
    solved_slots: Option<&[PlanTimeSlot]>,
) -> Vec<SessionFlexibility> {
    let step_s = planner.plan_step_s as i64;
    asset_contexts
        .iter()
        .filter_map(|ctx| {
            let window = ctx.flexibility_window(now)?;
            Some(session_entry(
                ctx.asset_id(),
                window,
                inputs,
                step_s,
                now,
                solved_slots,
            ))
        })
        .collect()
}
