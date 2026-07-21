use crate::entities::asset::{ComfortRate, CompletionPolicy};

// ── Battery ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct BatteryParams {
    pub id: String,
    pub capacity_kwh: f64,
    pub max_charge_kw: f64,
    pub max_discharge_kw: f64,
    pub initial_soc: f64,
    pub round_trip_efficiency: f64,
    pub min_soc: f64,
    /// Override for auto-computed terminal energy reward [EUR/kWh].
    /// None → auto-compute from avg import tariff × round_trip_efficiency.
    /// Some(0.0) → disabled. Some(x) → fixed at x EUR/kWh.
    pub c_terminal_eur_kwh: Option<f64>,
}

impl Default for BatteryParams {
    fn default() -> Self {
        Self {
            id: crate::ids::ASSET_BATTERY.to_string(),
            capacity_kwh: 10.0,
            max_charge_kw: 5.0,
            max_discharge_kw: 5.0,
            initial_soc: 0.5,
            round_trip_efficiency: 0.92,
            min_soc: 0.10,
            c_terminal_eur_kwh: None,
        }
    }
}

// ── EV ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct EvParams {
    pub id: String,
    pub max_charge_kw: f64,
    pub max_discharge_kw: f64,
    pub initial_soc: f64,
    pub battery_kwh: f64,
    pub soc_target: f64,
    pub default_charge_kw: f64,
    pub min_charge_kw: f64,
    /// BL-12: expected controller response delay (s), simulated as a single-tick lag.
    pub response_delay_s: f64,
}

impl Default for EvParams {
    fn default() -> Self {
        Self {
            id: crate::ids::ASSET_EV.to_string(),
            max_charge_kw: 7.4,
            max_discharge_kw: 0.0,
            initial_soc: 0.5,
            battery_kwh: 60.0,
            soc_target: 0.8,
            default_charge_kw: 0.0,
            min_charge_kw: 1.4,
            response_delay_s: 10.0,
        }
    }
}

// ── Heater ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct HeaterParams {
    pub id: String,
    pub max_kw: f64,
    pub temp_initial_c: f64,
    pub temp_min_c: f64,
    pub temp_max_c: f64,
    pub mid_kw: Option<f64>,
    pub thermal_mass_kwh_per_c: f64,
    pub k_loss_kw_per_c: f64,
    pub draw_kw: f64,
    pub switching_penalty_eur: f64,
    /// Override for auto-computed terminal energy reward [EUR/kWh].
    /// None → auto-compute from avg import tariff + c_ctrl_imp_malus.
    /// Some(0.0) → disabled. Some(x) → fixed at x EUR/kWh.
    pub c_terminal_eur_kwh: Option<f64>,
}

impl Default for HeaterParams {
    fn default() -> Self {
        Self {
            id: crate::ids::ASSET_HEATER.to_string(),
            max_kw: 5.0,
            temp_initial_c: 20.0,
            temp_min_c: 18.0,
            temp_max_c: 23.0,
            mid_kw: None,
            thermal_mass_kwh_per_c: 2.0,
            k_loss_kw_per_c: 0.1,
            draw_kw: 0.0,
            switching_penalty_eur: 0.01,
            c_terminal_eur_kwh: None,
        }
    }
}

// ── PV ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct PvParams {
    pub id: String,
    pub rated_kw: f64,
}

impl Default for PvParams {
    fn default() -> Self {
        Self {
            id: crate::ids::ASSET_PV.to_string(),
            rated_kw: 5.0,
        }
    }
}

impl PvParams {
    pub fn forecast_kw(&self, ts: chrono::DateTime<chrono::Utc>) -> f64 {
        use chrono::Timelike;
        let hour = ts.hour() as f64 + ts.minute() as f64 / 60.0;
        if (6.0..=18.0).contains(&hour) {
            let angle = std::f64::consts::PI * (hour - 6.0) / 12.0;
            (angle.sin().max(0.0) * self.rated_kw).max(0.0)
        } else {
            0.0
        }
    }
}

// ── PV weather-sourced forecast (additive — see PvParams above for the
//    sin-model path used by the simulator; this is the real-weather path,
//    docs/plans/weather-forecast-plugin.md) ──────────────────────────────────

/// PV array geometry, independent of electrical rating: geo-position, panel
/// tilt (0°=horizontal, 90°=vertical), and panel azimuth (compass bearing
/// the panel faces, 0°=N/90°=E/180°=S/270°=W).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PvArrayGeometry {
    pub location: crate::entities::weather::GeoPosition,
    pub tilt_deg: f64,
    pub azimuth_deg: f64,
}

/// Snow-cover model parameters — see `entities::pv_snow` for the state
/// machine these drive.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PvSnowParams {
    /// New snowfall this hour above this amount triggers full coverage.
    pub snowfall_trigger_cm: f64,
    /// Temperature above which a covered panel is assumed to self-clear
    /// within the hour.
    pub clear_threshold_c: f64,
    /// Output fraction while covered — usually 0.0.
    pub covered_output_fraction: f64,
}

impl Default for PvSnowParams {
    fn default() -> Self {
        Self {
            snowfall_trigger_cm: 0.2,
            clear_threshold_c: 1.5,
            covered_output_fraction: 0.0,
        }
    }
}

/// Full parameter set for the weather-sourced PV forecast
/// (`entities::solar::forecast_ac_kw`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PvForecastParams {
    /// DC nameplate rating at STC (1000 W/m², 25°C cell temp).
    pub rated_kwp: f64,
    pub geometry: PvArrayGeometry,
    /// System losses: inverter conversion, wiring, soiling, mismatch —
    /// everything NOT already captured by rated_kwp (module efficiency).
    pub performance_ratio: f64,
    /// Percent power change per °C above 25°C cell temperature (negative).
    pub temp_coeff_pct_per_c: f64,
    /// Nominal Operating Cell Temperature, for the cell-temperature model.
    pub noct_c: f64,
    /// Inverter AC clipping cap, if the AC rating is below rated_kwp.
    pub ac_limit_kw: Option<f64>,
    pub snow: PvSnowParams,
}

// ── BaseLoad ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct BaseLoadParams {
    pub id: String,
    pub baseline_kw: f64,
    pub spikes: Vec<ApplianceSpikeParams>,
}

impl Default for BaseLoadParams {
    fn default() -> Self {
        Self {
            id: crate::ids::ASSET_BASE_LOAD.to_string(),
            baseline_kw: 0.5,
            spikes: Vec::new(),
        }
    }
}

/// One simulated appliance draw bump (a trapezoidal pulse) — see
/// `profile::schema::SpikeConfig` for the YAML shape this is parsed from.
#[derive(Debug, Clone)]
pub struct ApplianceSpikeParams {
    pub center_hour: f64,
    pub jitter_h: f64,
    pub amplitude_kw: f64,
    /// Total on-period width in hours (ramp-up + plateau + ramp-down).
    pub duration_h: f64,
    /// Linear transition width at each edge, in hours.
    pub ramp_h: f64,
    pub probability: f64,
    /// `0`=Monday..`6`=Sunday; empty means every day.
    pub weekdays: Vec<u8>,
}

// ── Dispatch enum ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum AssetParams {
    Battery(BatteryParams),
    Ev(EvParams),
    Heater(HeaterParams),
    Pv(PvParams),
    BaseLoad(BaseLoadParams),
}

/// Minimal asset snapshot for user-request creation.
/// Built by the adapter layer (routes/hems.rs) from a locked SimState.
/// Pure domain type — no assets/ or simulator/ imports.
#[derive(Debug, Clone)]
pub struct AssetRequestSlice {
    pub id: String,
    /// Current SoC [0.0, 1.0] for storage assets; None for non-storage.
    pub current_soc: Option<f64>,
    /// Default SoC target when body.target_soc is None.
    pub default_soc_target: Option<f64>,
    /// Usable capacity in kWh; None for non-storage assets.
    pub capacity_kwh: Option<f64>,
    /// Max charge rate (kW); used as default desired_power when not specified.
    pub max_charge_kw: Option<f64>,
    pub completion_policy: CompletionPolicy,
    pub comfort_rates: Vec<ComfortRate>,
}

impl AssetRequestSlice {
    pub fn resolve_request_target(
        &self,
        target_soc: Option<f64>,
        desired_power_kw: Option<f64>,
    ) -> Option<(f64, f64)> {
        let current_soc = self.current_soc?;
        let capacity_kwh = self.capacity_kwh?;
        let target = target_soc.or(self.default_soc_target).unwrap_or(1.0);
        let kwh = (target - current_soc).max(0.0) * capacity_kwh;
        if kwh < 1e-6 {
            return None;
        }
        Some((kwh, desired_power_kw.or(self.max_charge_kw).unwrap_or(1.0)))
    }
}
