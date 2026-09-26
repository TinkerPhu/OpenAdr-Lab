//! Per-asset planned-state forecasts (T008/T013/T017): the battery SoC,
//! EV SoC and heater tank-temperature traces a solved plan carries on each
//! slot. Split out of `results.rs` to stay under the VEN/src/
//! 500-production-line cap (`ven-architecture` rule, `.claude/CLAUDE.md`).

use crate::entities::asset_params::{BatteryParams, EvParams, HeaterParams};
use crate::entities::plan::PlanTimeSlot;

use super::asset_port::{
    battery_future_state, ev_future_state_at, ev_soc_trajectory, heater_future_state,
};
use super::types::{MilpInputs, SolveOutput};

pub(super) fn fill_planned_state(
    slots: &mut [PlanTimeSlot],
    n: usize,
    sol: &SolveOutput,
    inputs: &MilpInputs,
    battery_cfg: Option<&BatteryParams>,
    ev_cfg: Option<&EvParams>,
    heat_cfg: Option<&HeaterParams>,
) {
    // Battery SoC forecast — e_bat_kwh[t] is start-of-slot stored energy.
    if let Some(bat_cfg) = battery_cfg {
        let capacity_kwh = bat_cfg.capacity_kwh;
        #[allow(clippy::needless_range_loop)] // t indexes both slots[] and sol.e_bat_kwh[]
        for t in 0..n {
            slots[t].planned_state_by_asset.insert(
                bat_cfg.id.clone(),
                battery_future_state(sol.e_bat_kwh[t], capacity_kwh),
            );
        }
    }
    // EV SoC forecast — requires soc_ev_init captured in MilpInputs, and folds in
    // `ev-usage-forecast`'s exogenous trip drops when the EV declared a forecast.
    if let (Some(soc_init), Some(ev_cfg)) = (inputs.soc_ev_init, ev_cfg) {
        let traj = ev_soc_trajectory(
            &sol.p_ev_kw,
            soc_init,
            ev_cfg.battery_kwh,
            &inputs.dt_h,
            inputs.ev_soc_drops.as_ref(),
        );
        #[allow(clippy::needless_range_loop)] // t indexes both slots[] and traj[]
        for t in 0..n {
            slots[t]
                .planned_state_by_asset
                .insert(ev_cfg.id.clone(), ev_future_state_at(traj[t]));
        }
    }
    // Heater T_tank forecast — e_heat_tank_kwh[t] is stored energy above temp_min_c.
    if let Some(heat_cfg) = heat_cfg {
        if !sol.e_heat_tank_kwh.is_empty() {
            let thermal_mass = heat_cfg.thermal_mass_kwh_per_c;
            let temp_min = heat_cfg.temp_min_c;
            #[allow(clippy::needless_range_loop)]
            // t indexes both slots[] and sol.e_heat_tank_kwh[]
            for t in 0..n {
                slots[t].planned_state_by_asset.insert(
                    heat_cfg.id.clone(),
                    heater_future_state(sol.e_heat_tank_kwh[t], temp_min, thermal_mass),
                );
            }
        }
    }
}
