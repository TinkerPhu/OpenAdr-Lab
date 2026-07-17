use chrono::{DateTime, Utc};

use crate::entities::asset_params::{EvParams, HeaterParams};
use crate::entities::device_session::ShiftableLoad;
use crate::entities::plan::{FlexibilityEnvelope, PlanTimeSlot};
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
        let dt_h = (slot.end - slot.start).num_seconds() as f64 / 3600.0;
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

/// Build per-device schedulability metadata for all active device sessions.
///
/// `solved_slots` is the translated schedule when a solution exists; estimates
/// are then derived from the actual allocations. Without it (fallback plan),
/// estimates degrade to energy × average import tariff over the window.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_plan_envelopes(
    ev_session: Option<&crate::entities::device_session::EvSession>,
    heater_target: Option<&crate::entities::device_session::HeaterTarget>,
    shiftable_loads: &[ShiftableLoad],
    inputs: &MilpInputs,
    planner: &PlannerParams,
    ev_cfg: Option<&EvParams>,
    heat_cfg: Option<&HeaterParams>,
    now: DateTime<Utc>,
    solved_slots: Option<&[PlanTimeSlot]>,
) -> Vec<FlexibilityEnvelope> {
    let step_s = planner.plan_step_s as i64;
    let n = inputs.n;
    let mut envelopes = Vec::new();

    // EV envelope
    if let Some(session) = ev_session {
        if let Some(ev_cfg) = ev_cfg {
            // Remaining energy to charge
            let energy_needed_kwh = inputs.e_ev_core_kwh;
            if energy_needed_kwh > 0.0 {
                let window_start = now;
                let window_end = session.departure_time;
                let slots_available =
                    ((window_end - window_start).num_seconds() / step_s).max(0) as usize;
                let t_start = 0usize;
                let t_end = ((window_end - now).num_seconds() / step_s)
                    .max(0)
                    .min(n as i64) as usize;
                let eligible = t_start..t_end;
                let count = eligible.len().max(1) as f64;
                let avg_tariff = (t_start..t_end)
                    .map(|t| inputs.c_imp_eur_kwh[t])
                    .sum::<f64>()
                    / count;
                let avg_co2 = (t_start..t_end)
                    .map(|t| inputs.g_imp_kgco2_kwh[t] * 1000.0)
                    .sum::<f64>()
                    / count;
                let (estimated_cost_eur, estimated_co2_g) = solved_slots
                    .and_then(|s| solved_session_cost(s, &ev_cfg.id, window_end))
                    .unwrap_or((energy_needed_kwh * avg_tariff, energy_needed_kwh * avg_co2));
                envelopes.push(FlexibilityEnvelope {
                    asset_id: ev_cfg.id.clone(),
                    energy_needed_kwh,
                    power_min_kw: ev_cfg.min_charge_kw,
                    power_max_kw: ev_cfg.max_charge_kw,
                    window_start,
                    window_end,
                    slots_available,
                    max_acceptable_rate: 0.35,
                    min_acceptable_rate: 0.05,
                    budget_remaining_eur: 1.0e9,
                    estimated_cost_eur,
                    estimated_co2_g,
                });
            }
        }
    }

    // Heater envelope
    if let Some(target) = heater_target {
        if let Some(heat_cfg) = heat_cfg {
            let energy_needed_kwh = inputs.e_heat_target_kwh;
            if energy_needed_kwh > 0.0 {
                let window_start = now;
                let window_end = target.ready_by;
                let slots_available =
                    ((window_end - window_start).num_seconds() / step_s).max(0) as usize;
                let t_end = ((window_end - now).num_seconds() / step_s)
                    .max(0)
                    .min(n as i64) as usize;
                let count = t_end.max(1) as f64;
                let avg_tariff = (0..t_end).map(|t| inputs.c_imp_eur_kwh[t]).sum::<f64>() / count;
                let avg_co2 = (0..t_end)
                    .map(|t| inputs.g_imp_kgco2_kwh[t] * 1000.0)
                    .sum::<f64>()
                    / count;
                let (estimated_cost_eur, estimated_co2_g) = solved_slots
                    .and_then(|s| solved_session_cost(s, &heat_cfg.id, window_end))
                    .unwrap_or((energy_needed_kwh * avg_tariff, energy_needed_kwh * avg_co2));
                envelopes.push(FlexibilityEnvelope {
                    asset_id: heat_cfg.id.clone(),
                    energy_needed_kwh,
                    power_min_kw: 0.0,
                    power_max_kw: inputs.p_heat_full_kw,
                    window_start,
                    window_end,
                    slots_available,
                    max_acceptable_rate: 0.35,
                    min_acceptable_rate: 0.05,
                    budget_remaining_eur: 1.0e9,
                    estimated_cost_eur,
                    estimated_co2_g,
                });
            }
        }
    }

    // Shiftable load envelopes
    for (s, sl) in shiftable_loads.iter().enumerate() {
        if s >= inputs.shiftable_loads.len() {
            break;
        }
        let milp_sl = &inputs.shiftable_loads[s];
        let energy_needed_kwh = sl.power_kw * sl.duration_min as f64 / 60.0;
        let window_start = sl.earliest_start.max(now);
        let window_end = sl.latest_end;
        let slots_available = ((window_end - window_start).num_seconds() / step_s).max(0) as usize;
        let t_start = ((window_start - now).num_seconds() / step_s).max(0) as usize;
        let t_end = ((window_end - now).num_seconds() / step_s)
            .max(0)
            .min(n as i64) as usize;
        let count = (t_end.saturating_sub(t_start)).max(1) as f64;
        let avg_tariff = (t_start..t_end)
            .map(|t| inputs.c_imp_eur_kwh[t])
            .sum::<f64>()
            / count;
        let avg_co2 = (t_start..t_end)
            .map(|t| inputs.g_imp_kgco2_kwh[t] * 1000.0)
            .sum::<f64>()
            / count;
        let _ = milp_sl;
        let (estimated_cost_eur, estimated_co2_g) = solved_slots
            .and_then(|s| solved_session_cost(s, &sl.asset_id, window_end))
            .unwrap_or((energy_needed_kwh * avg_tariff, energy_needed_kwh * avg_co2));
        envelopes.push(FlexibilityEnvelope {
            asset_id: sl.asset_id.clone(),
            energy_needed_kwh,
            power_min_kw: sl.power_kw,
            power_max_kw: sl.power_kw,
            window_start,
            window_end,
            slots_available,
            max_acceptable_rate: 0.35,
            min_acceptable_rate: 0.05,
            budget_remaining_eur: 1.0e9,
            estimated_cost_eur,
            estimated_co2_g,
        });
    }

    envelopes
}
