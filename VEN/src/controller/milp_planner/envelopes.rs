use chrono::{DateTime, Utc};

use crate::entities::device_session::ShiftableLoad;
use crate::entities::plan::FlexibilityEnvelope;
use crate::profile::Profile;

use super::types::*;

/// Build per-device schedulability metadata for all active device sessions.
pub(crate) fn build_plan_envelopes(
    ev_session: Option<&crate::entities::device_session::EvSession>,
    heater_target: Option<&crate::entities::device_session::HeaterTarget>,
    shiftable_loads: &[ShiftableLoad],
    inputs: &MilpInputs,
    profile: &Profile,
    now: DateTime<Utc>,
) -> Vec<FlexibilityEnvelope> {
    let step_s = profile.planner.plan_step_s as i64;
    let n = inputs.n;
    let mut envelopes = Vec::new();

    // EV envelope
    if let Some(session) = ev_session {
        if let Some(ev_cfg) = profile.ev_config() {
            // Remaining energy to charge
            let energy_needed_kwh = inputs.e_ev_core_kwh;
            if energy_needed_kwh > 0.0 {
                let window_start = now;
                let window_end = session.departure_time;
                let slots_available =
                    ((window_end - window_start).num_seconds() / step_s).max(0) as usize;
                let t_start = 0usize;
                let t_end = ((window_end - now).num_seconds() / step_s).min(n as i64) as usize;
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
                    estimated_cost_eur: energy_needed_kwh * avg_tariff,
                    estimated_co2_g: energy_needed_kwh * avg_co2,
                });
            }
        }
    }

    // Heater envelope
    if let Some(target) = heater_target {
        if let Some(heat_cfg) = profile.heater_config() {
            let energy_needed_kwh = inputs.e_heat_target_kwh;
            if energy_needed_kwh > 0.0 {
                let window_start = now;
                let window_end = target.ready_by;
                let slots_available =
                    ((window_end - window_start).num_seconds() / step_s).max(0) as usize;
                let t_end = ((window_end - now).num_seconds() / step_s).min(n as i64) as usize;
                let count = t_end.max(1) as f64;
                let avg_tariff = (0..t_end).map(|t| inputs.c_imp_eur_kwh[t]).sum::<f64>() / count;
                let avg_co2 = (0..t_end)
                    .map(|t| inputs.g_imp_kgco2_kwh[t] * 1000.0)
                    .sum::<f64>()
                    / count;
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
                    estimated_cost_eur: energy_needed_kwh * avg_tariff,
                    estimated_co2_g: energy_needed_kwh * avg_co2,
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
        let t_end = ((window_end - now).num_seconds() / step_s).min(n as i64) as usize;
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
            estimated_cost_eur: energy_needed_kwh * avg_tariff,
            estimated_co2_g: energy_needed_kwh * avg_co2,
        });
    }

    envelopes
}
