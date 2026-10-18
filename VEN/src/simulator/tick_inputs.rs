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

    // Heater
    pub ambient_temp_c_override: Option<f64>,
    pub heater_temp_min_override: Option<f64>,
    pub heater_temp_max_override: Option<f64>,
    /// `(curtail, absorb)` emergency-mode overrides per heater id (the arbiter's or an
    /// inject's); a heater without an entry follows its thermostat.
    pub heater_emergency_by_asset: HashMap<String, (Option<bool>, Option<bool>)>,

    // Base load
    pub base_load_kw_override: Option<f64>,
    pub base_load_alpha: f64,

    // EV
    pub ev_plugged_override: Option<bool>,
    pub ev_soc_target_override: Option<f64>,
    /// The departure of the session governing each EV now, by EV id.
    pub ev_departure_by_asset: HashMap<String, DateTime<Utc>>,

    /// Request targets per asset under the opportunistic objective (empty under every planning
    /// objective): an EV's session target, a heater's start-now ceiling. A sim inject of the
    /// same quantity wins.
    pub request_targets: crate::controller::arbiter::RequestTargets,

    /// This tick's external feeds, each for the asset it belongs to.
    pub feeds: AssetTickFeeds,
}

/// This tick's external feeds, per asset (W5b): each PV array's weather estimate and the
/// series behind it (`entities::solar::fresh_weather_pv_series`, from that array's own
/// geometry), the real measured reading of the asset a feed measures, and each base load's
/// learned heuristic. An asset without an entry has no such feed this tick.
#[derive(Debug, Clone, Default)]
pub struct AssetTickFeeds {
    pub weather_pv_kw: HashMap<String, f64>,
    pub weather_pv_forecast: HashMap<String, Vec<WeatherPvForecastSlot>>,
    pub measured_kw: HashMap<String, f64>,
    pub heuristic: HashMap<String, AssetHeuristics>,
}

impl AssetTickFeeds {
    /// `asset_id`'s learned heuristic sampled at `now`, if it has one.
    pub fn heuristic_kw(&self, asset_id: &str, now: DateTime<Utc>) -> Option<f64> {
        self.heuristic.get(asset_id).map(|h| h.sample_kw(now))
    }

    /// Only a weather estimate of `kw` for the array `asset_id`.
    #[cfg(test)]
    pub fn weather_kw(asset_id: &str, kw: f64) -> Self {
        Self {
            weather_pv_kw: HashMap::from([(asset_id.to_string(), kw)]),
            ..Self::default()
        }
    }

    /// Only a learned heuristic for `asset_id` that samples to `kw` at every hour.
    #[cfg(test)]
    pub fn heuristic_flat_kw(asset_id: &str, kw: f64) -> Self {
        let flat = AssetHeuristics {
            asset_id: asset_id.to_string(),
            daytime_profile_kw: std::array::from_fn(|_| vec![kw; 24]),
            seasonal_factor: 1.0,
            last_updated: None,
            recent_mean_abs_error_kw: None,
        };
        Self {
            heuristic: HashMap::from([(asset_id.to_string(), flat)]),
            ..Self::default()
        }
    }
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
            ambient_temp_c_override: None,
            heater_temp_min_override: None,
            heater_temp_max_override: None,
            heater_emergency_by_asset: HashMap::new(),
            base_load_kw_override: None,
            base_load_alpha: DEFAULT_BASE_LOAD_ALPHA,
            ev_plugged_override: None,
            ev_soc_target_override: None,
            ev_departure_by_asset: HashMap::new(),
            request_targets: Default::default(),
            feeds: AssetTickFeeds::default(),
        }
    }
}

/// Behaviour-B PV smoothing time constant [s] when nothing overrides it.
pub const DEFAULT_PV_TAU_S: f64 = 0.1;
/// Behaviour-B base-load EMA decay factor when nothing overrides it.
pub const DEFAULT_BASE_LOAD_ALPHA: f64 = 0.1;
