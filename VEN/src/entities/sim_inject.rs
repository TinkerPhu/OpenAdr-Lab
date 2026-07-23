use serde::{Deserialize, Serialize};

/// Simulation injection state — set via POST /sim/inject.
/// Three injection behaviours:
/// - A (one-shot): applied once to physics state, then cleared automatically.
/// - B (frozen + EMA return): held while active; EMA-blended back to natural model on release.
/// - C (frozen + snap): held while active; snaps to profile default on release.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SimInjectState {
    // Behaviour A — one-shot (cleared after application to physics state)
    pub battery_soc: Option<f64>,
    pub ev_soc: Option<f64>,
    pub heater_temp_c: Option<f64>,
    // Behaviour B — frozen + EMA return on release
    pub pv_irradiance: Option<f64>,
    pub pv_irradiance_alpha: f64,  // default 0.1
    pub base_load_kw: Option<f64>, // one-shot: offset stored in smoothing state, then cleared
    pub base_load_alpha: f64,      // default 0.1
    // Behaviour C — frozen while active, snap to profile default on release
    pub ev_plugged: Option<bool>,
    pub ev_soc_target: Option<f64>,
    pub heater_setpoint_c: Option<f64>,
    pub heater_temp_min_c: Option<f64>,
    pub heater_temp_max_c: Option<f64>,
    pub ambient_temp_c: Option<f64>,
    /// PV export ceiling (kW, positive magnitude); sign-converted to the
    /// internal `PvInverter.export_limit_kw` (≤ 0) convention where applied.
    pub pv_export_limit_kw: Option<f64>,
    // Behaviour D — planning-only override (no physics effect, no replan trigger)
    pub pv_plan_kw: Option<f64>,
}

impl Default for SimInjectState {
    fn default() -> Self {
        Self {
            battery_soc: None,
            ev_soc: None,
            heater_temp_c: None,
            pv_irradiance: None,
            pv_irradiance_alpha: 0.1,
            base_load_kw: None,
            base_load_alpha: 0.1,
            ev_plugged: None,
            ev_soc_target: None,
            heater_setpoint_c: None,
            heater_temp_min_c: None,
            heater_temp_max_c: None,
            ambient_temp_c: None,
            pv_export_limit_kw: None,
            pv_plan_kw: None,
        }
    }
}
