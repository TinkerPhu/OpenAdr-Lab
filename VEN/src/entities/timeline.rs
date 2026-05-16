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
    pub fn next_slot(&mut self, p_heat_kw: f64, dt_h: f64) -> HashMap<String, f64> {
        let temp_c = self.temp_min_c + self.e_kwh / self.thermal_mass;
        self.e_kwh = (self.e_kwh + (p_heat_kw - self.q_dem_kw) * dt_h).clamp(0.0, self.e_max_kwh);
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
