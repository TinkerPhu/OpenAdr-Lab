use crate::entities::asset_params::{
    ApplianceSpikeParams, AssetParams, BaseLoadParams, BatteryParams, EvParams, HeaterParams,
    PvForecastParams, PvParams,
};
use crate::profile::weather_pv::WeatherPvConfig;
use serde::Deserialize;

/// The `planner:` block lives in `profile::planner` (moved there to keep this
/// file under the production-line cap); re-exported so existing
/// `profile::schema::PlannerConfig` paths keep resolving.
pub use super::planner::PlannerConfig;

/// YAML-loaded asset profile tagged enum for the `assets:` list format.
/// Each entry has a `type` discriminator plus type-specific fields.
/// Renamed from `AssetConfig` in Phase A to avoid collision with `assets::AssetConfig`
/// (runtime physics dispatch enum).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AssetProfile {
    Ev(EvConfig),
    Heater(HeaterConfig),
    Pv(PvConfig),
    Battery(BatteryConfig),
    BaseLoad(BaseLoadConfig),
}

impl AssetProfile {
    /// Convert this config variant into the domain-level `AssetParams`.
    /// Called at startup only — not on the hot path.
    pub fn to_params(&self) -> AssetParams {
        match self {
            AssetProfile::Battery(c) => AssetParams::Battery(BatteryParams {
                id: c.id.clone(),
                capacity_kwh: c.capacity_kwh,
                max_charge_kw: c.max_charge_kw,
                max_discharge_kw: c.max_discharge_kw,
                initial_soc: c.initial_soc,
                round_trip_efficiency: c.round_trip_efficiency,
                min_soc: c.min_soc,
                c_terminal_eur_kwh: c.c_terminal_eur_kwh,
            }),
            AssetProfile::Ev(c) => AssetParams::Ev(EvParams {
                id: c.id.clone(),
                max_charge_kw: c.max_charge_kw,
                max_discharge_kw: c.max_discharge_kw,
                initial_soc: c.initial_soc,
                battery_kwh: c.battery_kwh,
                soc_target: c.soc_target,
                default_charge_kw: c.default_charge_kw,
                min_charge_kw: c.min_charge_kw,
                response_delay_s: c.response_delay_s,
                v2g_capable: c.v2g_capable,
            }),
            AssetProfile::Heater(c) => AssetParams::Heater(HeaterParams {
                id: c.id.clone(),
                max_kw: c.max_kw,
                temp_initial_c: c.temp_initial_c,
                temp_min_c: c.temp_min_c,
                temp_max_c: c.temp_max_c,
                temp_safety_max_c: c.temp_safety_max_c.unwrap_or(c.temp_max_c),
                power_stages: c.power_stages,
                thermal_mass_kwh_per_c: c.effective_thermal_mass(),
                k_loss_kw_per_c: c.effective_k_loss(),
                draw_kw: c.effective_draw_kw(),
                switching_penalty_eur: c.effective_switching_penalty(),
                c_terminal_eur_kwh: c.c_terminal_eur_kwh,
            }),
            AssetProfile::Pv(c) => AssetParams::Pv(PvParams {
                id: c.id.clone(),
                rated_kw: c.rated_kw,
                inverter_max_kw: c.inverter_max_kw.unwrap_or(c.rated_kw),
                co2_g_kwh: c.co2_g_kwh,
            }),
            AssetProfile::BaseLoad(c) => AssetParams::BaseLoad(BaseLoadParams {
                id: c.id.clone(),
                baseline_kw: c.baseline_kw,
                spikes: c
                    .spikes
                    .iter()
                    .map(|s| ApplianceSpikeParams {
                        center_hour: s.center_hour,
                        jitter_h: s.jitter_h,
                        amplitude_kw: s.amplitude_kw,
                        duration_h: s.duration_h,
                        ramp_h: s.ramp_h,
                        probability: s.probability,
                        weekdays: s.weekdays.clone(),
                    })
                    .collect(),
            }),
        }
    }
}

impl Profile {
    /// Convert all asset profiles to domain `AssetParams` (one allocation at startup).
    pub fn asset_params(&self) -> Vec<AssetParams> {
        self.assets.iter().map(|ap| ap.to_params()).collect()
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Profile {
    /// New typed asset list format.
    #[serde(default)]
    pub assets: Vec<AssetProfile>,
    #[serde(default)]
    pub simulator: SimulatorConfig,
    #[serde(default)]
    pub planner: PlannerConfig,
    #[serde(default)]
    pub grid: super::grid::GridConfig,
    #[serde(default)]
    pub history: HistoryConfig,
    /// Weather-sourced PV forecast config (weather-forecast-visibility).
    /// Optional and additive — absent by default, so every profile without
    /// it parses and behaves exactly as before this section existed.
    #[serde(default)]
    pub weather_pv: Option<WeatherPvConfig>,
    /// Real-measurement MQTT feed enable flags (per-VEN gate, alongside the
    /// env-var/transport gate — both must allow a signal for it to take
    /// effect). Optional and additive — absent by default.
    #[serde(default)]
    pub measurements: Option<MeasurementsConfig>,
    /// GB-09: per-VEN VTN poll cadence + startup jitter. Omitted -> 30/30/60s, zero-jitter defaults.
    #[serde(default)]
    pub polling: super::polling::PollConfig,
    /// R-59: VTN-communication-loss power curtailment. Optional and
    /// additive — absent by default, so every profile without it parses and
    /// behaves exactly as before this section existed.
    #[serde(default)]
    pub comms_loss: Option<super::comms_loss::CommsLossConfig>,
}

impl Profile {
    /// Weather-sourced PV forecast params, if configured. `None` when the
    /// profile has no `weather_pv` section — callers (the `GET /weather`
    /// route) treat that as "derived state unavailable," not an error.
    pub fn weather_pv_params(&self) -> Option<PvForecastParams> {
        self.weather_pv.as_ref().map(WeatherPvConfig::to_params)
    }

    /// Whether this VEN's profile enables trusting a real measured PV
    /// reading. `false` when the profile has no `measurements` section or
    /// omits `pv_enabled` — the profile-level half of the two-gate design
    /// (the other half is the `PV_MEASUREMENT_MQTT_HOST` env var).
    pub fn pv_measurement_enabled(&self) -> bool {
        self.measurements.as_ref().is_some_and(|m| m.pv_enabled)
    }

    /// Whether this VEN's profile enables trusting a real measured baseline
    /// load reading. Same two-gate design as `pv_measurement_enabled`.
    pub fn base_load_measurement_enabled(&self) -> bool {
        self.measurements
            .as_ref()
            .is_some_and(|m| m.base_load_enabled)
    }
}

/// `measurements:` profile section — enable flags only; no physical params
/// needed here since the asset structs already carry those (`rated_kw`
/// etc.). See docs/architecture for the two-gate design (env var + profile).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct MeasurementsConfig {
    #[serde(default)]
    pub pv_enabled: bool,
    #[serde(default)]
    pub base_load_enabled: bool,
}

/// WP1.2/WP1.3 (Phase 1, A-1) — persistent history sampling + retention.
#[derive(Debug, Clone, Deserialize)]
pub struct HistoryConfig {
    #[serde(default = "super::defaults::default_history_enabled")]
    pub enabled: bool,
    #[serde(default = "super::defaults::default_history_retention_days")]
    pub retention_days: u32,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            enabled: super::defaults::default_history_enabled(),
            retention_days: super::defaults::default_history_retention_days(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct EvConfig {
    #[serde(default = "super::defaults::default_asset_id_ev")]
    pub id: String,
    #[serde(default = "super::defaults::default_ev_max_charge")]
    pub max_charge_kw: f64,
    #[serde(default = "super::defaults::default_ev_max_discharge")]
    pub max_discharge_kw: f64,
    #[serde(default = "super::defaults::default_ev_soc")]
    pub initial_soc: f64,
    #[serde(default = "super::defaults::default_ev_battery")]
    pub battery_kwh: f64,
    #[serde(default = "super::defaults::default_ev_soc_target")]
    pub soc_target: f64,
    #[serde(default)]
    pub default_charge_kw: f64,
    /// Minimum charge power when plugged in (kW). EVSE semi-continuous lower bound:
    /// if charging at all, power must be at least this value (no trickle charging).
    /// Typical EVSE minimum: 6 A × 230 V ≈ 1.4 kW.
    #[serde(default = "super::defaults::default_ev_min_charge")]
    pub min_charge_kw: f64,
    /// BL-12: expected controller response delay (s), simulated as a single-tick lag.
    #[serde(default = "super::defaults::default_ev_response_delay")]
    pub response_delay_s: f64,
    /// Whether this EV's EVSE hardware actually supports bidirectional
    /// (vehicle-to-grid) discharge. Defaults to false — most EVSE installs
    /// are charge-only, so `max_discharge_kw` is otherwise inert.
    #[serde(default = "super::defaults::default_ev_v2g_capable")]
    pub v2g_capable: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HeaterConfig {
    #[serde(default = "super::defaults::default_asset_id_heater")]
    pub id: String,
    #[serde(default = "super::defaults::default_heater_max")]
    pub max_kw: f64,
    #[serde(default = "super::defaults::default_heater_temp")]
    pub temp_initial_c: f64,
    #[serde(default = "super::defaults::default_heater_min")]
    pub temp_min_c: f64,
    #[serde(default = "super::defaults::default_heater_max_temp")]
    pub temp_max_c: f64,
    /// True hard safety ceiling (°C), above `temp_max_c` — e.g. scalding risk / relief-valve
    /// limit for a hot-water tank. Only reachable in `HeaterEmergencyMode::Absorb`, which is
    /// not wired to a live VTN signal yet (sim-inject only). Defaults to `temp_max_c` (no
    /// extra headroom) when omitted, so existing profiles are unaffected.
    #[serde(default)]
    pub temp_safety_max_c: Option<f64>,
    /// Number of switchable power stages: 1 (on/off) or 2 (mid/full). Levels are
    /// always evenly spaced at `max_kw / power_stages`, which is the physics of a
    /// staged resistive element — there is deliberately no free mid-power field, so
    /// a profile cannot express an unreachable level. Default: 2.
    #[serde(default = "super::defaults::default_power_stages")]
    pub power_stages: u8,
    /// Tank volume in litres. If set, thermal mass = `volume_l × 4.186 / 3600` kWh/°C.
    /// Takes precedence over `thermal_mass_kwh_per_c`. For a 200 L water tank: ~0.233 kWh/°C.
    #[serde(default)]
    pub volume_l: Option<f64>,
    /// Explicit thermal mass (kWh/°C). Used when `volume_l` is not set.
    /// Defaults to 2.0 kWh/°C (legacy space-heater value) for backward compatibility.
    #[serde(default)]
    pub thermal_mass_kwh_per_c: Option<f64>,
    /// Newton cooling coefficient (kW/°C). Determines heat loss rate:
    /// `loss_kw = k_loss_kw_per_c × (temp_c − ambient_temp_c)`.
    /// Defaults to 0.1 kW/°C (legacy space-heater value).
    /// For a well-insulated 200 L hot water tank, a typical value is 0.003–0.005 kW/°C.
    #[serde(default)]
    pub k_loss_kw_per_c: Option<f64>,
    /// Constant simulated hot water draw (kW thermal). Models daily usage by removing
    /// thermal energy from the tank at a steady rate.
    /// Defaults to 0.0 (no draw — backward compatible).
    #[serde(default)]
    pub draw_kw: Option<f64>,
    /// Relay switching penalty coefficient [EUR/switch event] used in the MILP objective.
    /// Penalises each mode change to reduce relay wear.
    /// Defaults to 0.01 EUR/switch when absent.
    #[serde(default)]
    pub switching_penalty_eur: Option<f64>,
    /// Override for auto-computed terminal energy reward [EUR/kWh].
    /// Omit (or None) → auto-compute from mean(c_imp) + c_ctrl_imp_malus.
    /// 0.0 → disabled. Any positive value → fixed coefficient.
    #[serde(default)]
    pub c_terminal_eur_kwh: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PvConfig {
    #[serde(default = "super::defaults::default_asset_id_pv")]
    pub id: String,
    #[serde(default = "super::defaults::default_pv_rated")]
    pub rated_kw: f64,
    /// Inverter's true AC output capability (kW), distinct from `rated_kw` (installed DC panel
    /// peak) — real installations routinely run an inverter rated below panel peak (deliberate
    /// DC/AC oversizing). Defaults to `rated_kw` when omitted, so existing profiles are unaffected.
    #[serde(default)]
    pub inverter_max_kw: Option<f64>,
    /// PV embodied carbon, gCO2eq/kWh — reporting-only, not in the planner objective.
    #[serde(default = "super::defaults::default_pv_co2_g_kwh")]
    pub co2_g_kwh: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BatteryConfig {
    #[serde(default = "super::defaults::default_asset_id_battery")]
    pub id: String,
    #[serde(default = "super::defaults::default_battery_capacity")]
    pub capacity_kwh: f64,
    #[serde(default = "super::defaults::default_battery_charge")]
    pub max_charge_kw: f64,
    #[serde(default = "super::defaults::default_battery_discharge")]
    pub max_discharge_kw: f64,
    #[serde(default = "super::defaults::default_battery_soc")]
    pub initial_soc: f64,
    #[serde(default = "super::defaults::default_battery_efficiency")]
    pub round_trip_efficiency: f64,
    #[serde(default = "super::defaults::default_battery_min_soc")]
    pub min_soc: f64,
    /// Optional override for auto-computed terminal energy reward [EUR/kWh].
    /// None (omitted in YAML): auto-compute from avg import tariff × round_trip_efficiency.
    /// Some(0.0): disabled. Some(x): fixed at x EUR/kWh.
    #[serde(default)]
    pub c_terminal_eur_kwh: Option<f64>,
}

/// Base load fixed background consumption.
#[derive(Debug, Clone, Deserialize)]
pub struct BaseLoadConfig {
    #[serde(default = "super::defaults::default_asset_id_base_load")]
    pub id: String,
    #[serde(default = "super::defaults::default_base_load_kw")]
    pub baseline_kw: f64,
    /// Simulated appliance draw bumps (coffee/cooking/TV etc.). Empty by
    /// default — profiles that don't list any get zero appliance noise.
    #[serde(default)]
    pub spikes: Vec<SpikeConfig>,
}

/// One simulated appliance draw: a Gaussian-shaped power bump centered on
/// `center_hour`, with day-to-day jitter in timing and magnitude, and a
/// `probability` (0.0-1.0) that it fires at all on a given simulated day.
#[derive(Debug, Clone, Deserialize)]
pub struct SpikeConfig {
    pub center_hour: f64,
    #[serde(default = "super::defaults::default_spike_jitter_h")]
    pub jitter_h: f64,
    pub amplitude_kw: f64,
    /// Total on-period width in hours (ramp-up + plateau + ramp-down).
    #[serde(default = "super::defaults::default_spike_duration_h")]
    pub duration_h: f64,
    /// Linear transition width at each edge, in hours.
    #[serde(default = "super::defaults::default_spike_ramp_h")]
    pub ramp_h: f64,
    #[serde(default = "super::defaults::default_spike_probability")]
    pub probability: f64,
    /// `0`=Monday..`6`=Sunday; empty (the default) means every day.
    #[serde(default)]
    pub weekdays: Vec<u8>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SimulatorConfig {
    #[serde(default = "super::defaults::default_tick")]
    pub tick_s: u64,
    #[serde(default = "super::defaults::default_persist_every")]
    pub persist_every_s: u64,
    #[serde(default = "super::defaults::default_report_interval")]
    pub report_interval_s: u64,
}
