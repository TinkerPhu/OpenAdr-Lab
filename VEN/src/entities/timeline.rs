use chrono::{DateTime, Utc};
use std::collections::HashMap;

use crate::entities::asset::AssetType;

/// One sampled moment in an asset's history — domain-side type, no infra deps.
pub struct TimelinePoint {
    pub ts: DateTime<Utc>,
    pub power_kw: f64,
    pub state_values: HashMap<String, f64>,
}

/// Stateful temperature trajectory computer for plan display — domain-side, no infra deps.
/// Construction happens in `to_timeline_snapshot()` (infra layer).
#[derive(Clone)]
pub struct HeaterPlanTrajectory {
    pub e_kwh: f64,
    pub temp_min_c: f64,
    pub thermal_mass: f64,
    pub q_dem_kw: f64,
    pub e_max_kwh: f64,
}

impl HeaterPlanTrajectory {
    /// Returns state values for the start of this slot, then advances internal energy.
    /// Lower bound mirrors the MILP domain: e_lo = −max(e_max, 1.0), same as heater.rs declare_vars.
    /// Without this, e_kwh floors at 0 (= T_min) and heat-loss below T_min becomes invisible.
    pub fn next_slot(&mut self, p_heat_kw: f64, dt_h: f64) -> HashMap<String, f64> {
        let temp_c = self.temp_min_c + self.e_kwh / self.thermal_mass;
        let e_lo = -(self.e_max_kwh.max(1.0));
        self.e_kwh = (self.e_kwh + (p_heat_kw - self.q_dem_kw) * dt_h).clamp(e_lo, self.e_max_kwh);
        HashMap::from([("temp_c".into(), temp_c)])
    }
}

/// Per-asset data needed to render timelines — domain-only, no infra deps.
pub struct TimelineAssetData {
    pub asset_id: String,
    pub asset_type: AssetType,
    pub history: Vec<TimelinePoint>,
    pub current_power_kw: f64,
    pub current_state_values: HashMap<String, f64>,
    pub plan_trajectory: Option<HeaterPlanTrajectory>,
}

/// A decoupled snapshot of all data needed to render asset timelines.
///
/// Created by `SimState::to_timeline_snapshot()` while holding the sim lock;
/// the lock is then released before the (potentially slow) timeline rendering.
pub struct TimelineSnapshot {
    pub assets: HashMap<String, TimelineAssetData>,
    pub grid_history: Vec<TimelinePoint>,
    pub grid_current_kw: f64,
}

/// Time window parameters for `build_asset_timeline`.
pub struct TimeWindow {
    /// Hours of history to include (clamped to ≥ 0).
    pub hours_back: f64,
    /// Hours of future plan to include (clamped to ≥ 0).
    pub hours_forward: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Before fix: e_kwh was clamped at 0.0, so when starting at T_min with heater off,
    /// every call to next_slot returned exactly T_min (flat 45 °C). The heat loss and draw
    /// were consumed but e_kwh never went negative, hiding the thermal decline.
    #[test]
    fn test_next_slot_temperature_declines_below_t_min_when_heater_off() {
        let mut traj = HeaterPlanTrajectory {
            e_kwh: 0.0, // tank exactly at T_min
            temp_min_c: 45.0,
            thermal_mass: 0.23256,
            q_dem_kw: 0.5125, // ven-3 production value: draw + loss at T_min
            e_max_kwh: 3.489, // (60-45)*0.23256
        };
        let dt_h = 300.0 / 3600.0; // 5-min slot (Zone A)

        let s0 = traj.next_slot(0.0, dt_h);
        let s1 = traj.next_slot(0.0, dt_h);

        assert!(
            s0["temp_c"] <= 45.0,
            "slot 0 must return T_min (energy was 0 at start): got {:.4}",
            s0["temp_c"],
        );
        assert!(
            s1["temp_c"] < 45.0,
            "slot 1 temp must fall below T_min when heater off: got {:.4} (was flat before fix)",
            s1["temp_c"],
        );
    }
}
