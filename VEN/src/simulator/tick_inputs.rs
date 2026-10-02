//! Everything one simulator tick is told about the outside world.
//!
//! `SimState::tick` took these as 25 positional parameters, 20 of them
//! `Option`, which made every call site a column of bare `None`s whose
//! meaning depended on counting. Adding an input meant editing 27 call sites
//! and hoping each new `None` landed in the right slot.
//!
//! Distinct from `assets::TickOverrides`, which is what reaches the assets
//! *after* `tick` resolves the cross-asset terms (PV/base-load smoothing).
//! This is the raw, pre-resolution side: what the tick task read from
//! `TickContext` and the inject state.

use chrono::{DateTime, Utc};
use std::collections::HashMap;

use crate::entities::asset_params::PvCurtailmentSource;
use crate::entities::design_vocabulary::AssetHeuristics;
use crate::entities::solar::WeatherPvForecastSlot;

pub struct TickInputs {
    pub dt_s: f64,
    pub now: DateTime<Utc>,
    pub setpoints: HashMap<String, f64>,

    // PV
    pub pv_irradiance_override: Option<f64>,
    pub pv_tau_s: f64,
    pub pv_generation_limit_override: Option<f64>,
    pub pv_curtailment_source: PvCurtailmentSource,
    pub pv_measured_kw: Option<f64>,
    pub weather_pv_kw: Option<f64>,
    pub weather_pv_forecast: Option<Vec<WeatherPvForecastSlot>>,

    // Heater
    pub ambient_temp_c_override: Option<f64>,
    pub heater_temp_min_override: Option<f64>,
    pub heater_temp_max_override: Option<f64>,
    pub heater_emergency_curtail_override: Option<bool>,
    pub heater_emergency_absorb_override: Option<bool>,

    // Base load
    pub base_load_kw_override: Option<f64>,
    pub base_load_alpha: f64,
    pub base_load_measured_kw: Option<f64>,
    pub base_load_heuristic_kw: Option<f64>,
    pub base_load_heuristic: Option<AssetHeuristics>,

    // EV
    pub ev_plugged_override: Option<bool>,
    pub ev_soc_target_override: Option<f64>,
    pub ev_departure_time: Option<DateTime<Utc>>,
}

impl TickInputs {
    /// The three inputs every tick has, with no overrides and no external
    /// feeds — "nothing unusual is happening". Callers set only the fields
    /// they actually mean, through struct-update syntax:
    ///
    /// ```ignore
    /// sim.tick(TickInputs {
    ///     pv_irradiance_override: Some(0.8),
    ///     ..TickInputs::new(dt_s, now, setpoints)
    /// });
    /// ```
    pub fn new(dt_s: f64, now: DateTime<Utc>, setpoints: HashMap<String, f64>) -> Self {
        Self {
            dt_s,
            now,
            setpoints,
            pv_irradiance_override: None,
            // The smoothing time constant and the base-load EMA factor are
            // not overrides: a tick always has them. These are the defaults
            // every call site passed literally.
            pv_tau_s: DEFAULT_PV_TAU_S,
            pv_generation_limit_override: None,
            pv_curtailment_source: PvCurtailmentSource::None,
            pv_measured_kw: None,
            weather_pv_kw: None,
            weather_pv_forecast: None,
            ambient_temp_c_override: None,
            heater_temp_min_override: None,
            heater_temp_max_override: None,
            heater_emergency_curtail_override: None,
            heater_emergency_absorb_override: None,
            base_load_kw_override: None,
            base_load_alpha: DEFAULT_BASE_LOAD_ALPHA,
            base_load_measured_kw: None,
            base_load_heuristic_kw: None,
            base_load_heuristic: None,
            ev_plugged_override: None,
            ev_soc_target_override: None,
            ev_departure_time: None,
        }
    }
}

/// Behaviour-B PV smoothing time constant [s] when nothing overrides it.
pub const DEFAULT_PV_TAU_S: f64 = 0.1;
/// Behaviour-B base-load EMA decay factor when nothing overrides it.
pub const DEFAULT_BASE_LOAD_ALPHA: f64 = 0.1;
