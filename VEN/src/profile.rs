use serde::Deserialize;
use std::path::Path;
use tracing::{info, warn};

use crate::controller::flexibility_policy::FlexibilityPolicy;

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
    /// Asset identifier — must be present in every YAML entry.
    pub fn id(&self) -> &str {
        match self {
            Self::Ev(c) => &c.id,
            Self::Heater(c) => &c.id,
            Self::Pv(c) => &c.id,
            Self::Battery(c) => &c.id,
            Self::BaseLoad(c) => &c.id,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Profile {
    /// Legacy named-device format (old YAML). Used during transition.
    #[serde(default)]
    pub devices: DeviceConfig,
    /// New typed asset list format.
    #[serde(default)]
    pub assets: Vec<AssetProfile>,
    #[serde(default)]
    pub reactor: ReactorConfig,
    #[serde(default)]
    pub simulator: SimulatorConfig,
    #[serde(default)]
    pub planner: PlannerConfig,
    #[serde(default)]
    pub packets: Vec<PacketSeed>,
    #[serde(default)]
    pub flexibility_policy: FlexibilityPolicy,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DeviceConfig {
    pub ev: Option<EvConfig>,
    pub heater: Option<HeaterConfig>,
    pub pv: Option<PvConfig>,
    pub battery: Option<BatteryConfig>,
    #[serde(default = "default_base_load")]
    pub base_load_w: f64,
}

fn default_base_load() -> f64 {
    500.0
}

#[derive(Debug, Clone, Deserialize)]
pub struct EvConfig {
    #[serde(default = "default_asset_id_ev")]
    pub id: String,
    #[serde(default = "default_ev_max_charge")]
    pub max_charge_kw: f64,
    #[serde(default = "default_ev_max_discharge")]
    pub max_discharge_kw: f64,
    #[serde(default = "default_ev_soc")]
    pub initial_soc: f64,
    #[serde(default = "default_ev_battery")]
    pub battery_kwh: f64,
    #[serde(default = "default_ev_soc_target")]
    pub soc_target: f64,
    #[serde(default)]
    pub default_charge_kw: f64,
}

fn default_asset_id_ev() -> String {
    "ev".into()
}

fn default_ev_max_charge() -> f64 {
    7.4
}
fn default_ev_max_discharge() -> f64 {
    0.0
}
fn default_ev_soc() -> f64 {
    0.5
}
fn default_ev_battery() -> f64 {
    60.0
}
fn default_ev_soc_target() -> f64 {
    0.8
}

#[derive(Debug, Clone, Deserialize)]
pub struct HeaterConfig {
    #[serde(default = "default_asset_id_heater")]
    pub id: String,
    #[serde(default = "default_heater_max")]
    pub max_kw: f64,
    #[serde(default = "default_heater_temp")]
    pub temp_initial_c: f64,
    #[serde(default = "default_heater_min")]
    pub temp_min_c: f64,
    #[serde(default = "default_heater_max_temp")]
    pub temp_max_c: f64,
}

fn default_asset_id_heater() -> String {
    "heater".into()
}

fn default_heater_max() -> f64 {
    5.0
}
fn default_heater_temp() -> f64 {
    20.0
}
fn default_heater_min() -> f64 {
    18.0
}
fn default_heater_max_temp() -> f64 {
    23.0
}

#[derive(Debug, Clone, Deserialize)]
pub struct PvConfig {
    #[serde(default = "default_asset_id_pv")]
    pub id: String,
    #[serde(default = "default_pv_rated")]
    pub rated_kw: f64,
}

fn default_asset_id_pv() -> String {
    "pv".into()
}
fn default_pv_rated() -> f64 {
    5.0
}

#[derive(Debug, Clone, Deserialize)]
pub struct BatteryConfig {
    #[serde(default = "default_asset_id_battery")]
    pub id: String,
    #[serde(default = "default_battery_capacity")]
    pub capacity_kwh: f64,
    #[serde(default = "default_battery_charge")]
    pub max_charge_kw: f64,
    #[serde(default = "default_battery_discharge")]
    pub max_discharge_kw: f64,
    #[serde(default = "default_battery_soc")]
    pub initial_soc: f64,
    #[serde(default = "default_battery_efficiency")]
    pub round_trip_efficiency: f64,
    #[serde(default = "default_battery_min_soc")]
    pub min_soc: f64,
}

fn default_asset_id_battery() -> String {
    "battery".into()
}

fn default_battery_capacity() -> f64 {
    10.0
}
fn default_battery_charge() -> f64 {
    5.0
}
fn default_battery_discharge() -> f64 {
    5.0
}
fn default_battery_soc() -> f64 {
    0.5
}
fn default_battery_efficiency() -> f64 {
    0.92
}
fn default_battery_min_soc() -> f64 {
    0.10
}

/// Base load fixed background consumption.
#[derive(Debug, Clone, Deserialize)]
pub struct BaseLoadConfig {
    #[serde(default = "default_asset_id_base_load")]
    pub id: String,
    #[serde(default = "default_base_load_kw")]
    pub baseline_kw: f64,
}

fn default_asset_id_base_load() -> String {
    "base_load".into()
}
fn default_base_load_kw() -> f64 {
    0.5
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ReactorConfig {
    #[serde(default = "default_strategy")]
    pub strategy: String,
    #[serde(default = "default_ramp_duration")]
    pub ramp_duration_s: u64,
    #[serde(default)]
    pub delay_s: u64,
    #[serde(default = "default_compliance")]
    pub compliance: f64,
    #[serde(default = "default_price_low")]
    pub price_low: f64,
    #[serde(default = "default_price_high")]
    pub price_high: f64,
}

fn default_strategy() -> String {
    "instant".into()
}
fn default_ramp_duration() -> u64 {
    300
}
fn default_compliance() -> f64 {
    1.0
}
fn default_price_low() -> f64 {
    0.10
}
fn default_price_high() -> f64 {
    0.35
}

#[derive(Debug, Clone, Deserialize)]
pub struct SimulatorConfig {
    #[serde(default = "default_tick")]
    pub tick_s: u64,
    #[serde(default = "default_persist_every")]
    pub persist_every_s: u64,
    #[serde(default = "default_report_interval")]
    pub report_interval_s: u64,
}

impl Default for SimulatorConfig {
    fn default() -> Self {
        Self {
            tick_s: default_tick(),
            persist_every_s: default_persist_every(),
            report_interval_s: default_report_interval(),
        }
    }
}

fn default_tick() -> u64 {
    1
}
fn default_persist_every() -> u64 {
    15
}
fn default_report_interval() -> u64 {
    60
}

/// Configuration for the HEMS Planner (Stage 3).
#[derive(Debug, Clone, Deserialize)]
pub struct PlannerConfig {
    /// Planning timestep in seconds (default 300 = 5 min).
    #[serde(default = "default_plan_step")]
    pub plan_step_s: u64,
    /// Near-horizon window in hours — slots here become FIRM (default 4).
    #[serde(default = "default_near_horizon_h")]
    pub near_horizon_h: u64,
    /// Total planning horizon in hours (default 24).
    #[serde(default = "default_plan_horizon_h")]
    pub plan_horizon_h: u64,
    /// Seconds between periodic replanning cycles (default 300).
    #[serde(default = "default_replan_interval")]
    pub replan_interval_s: u64,
    /// Lookahead window for capability/tariff precomputation (hours, default 2.0).
    #[serde(default = "default_lookahead_h")]
    pub lookahead_h: f64,
}

impl Default for PlannerConfig {
    fn default() -> Self {
        Self {
            plan_step_s: default_plan_step(),
            near_horizon_h: default_near_horizon_h(),
            plan_horizon_h: default_plan_horizon_h(),
            replan_interval_s: default_replan_interval(),
            lookahead_h: default_lookahead_h(),
        }
    }
}

fn default_plan_step() -> u64 {
    300
}
fn default_near_horizon_h() -> u64 {
    4
}
fn default_plan_horizon_h() -> u64 {
    24
}
fn default_replan_interval() -> u64 {
    300
}
fn default_lookahead_h() -> f64 {
    2.0
}

/// A single comfort-rate point for a seeded packet.
#[derive(Debug, Clone, Deserialize)]
pub struct ComfortRateSeed {
    pub fill: f64,
    pub bid: f64,
}

/// An EnergyPacket pre-configured in the device profile (seeded at startup).
#[derive(Debug, Clone, Deserialize)]
pub struct PacketSeed {
    /// Asset id this packet targets: "ev", "heater", etc.
    pub asset: String,
    /// SoC target (0.0–1.0) for battery-like assets.
    pub target_soc: Option<f64>,
    /// Hours from VEN startup until this packet's deadline.
    pub latest_end_h: f64,
    /// Preferred charge power (kW); defaults to asset's max_charge_kw.
    pub desired_power_kw: Option<f64>,
    /// Comfort rates for ValueCurve (fill→bid pairs, ascending fill).
    #[serde(default)]
    pub comfort_rates: Vec<ComfortRateSeed>,
}

impl Profile {
    /// Returns the EV config: checks `assets` list first, falls back to legacy `devices`.
    pub fn ev_config(&self) -> Option<&EvConfig> {
        self.assets
            .iter()
            .find_map(|a| {
                if let AssetProfile::Ev(c) = a {
                    Some(c)
                } else {
                    None
                }
            })
            .or(self.devices.ev.as_ref())
    }

    /// Returns the Heater config: checks `assets` list first, falls back to legacy `devices`.
    pub fn heater_config(&self) -> Option<&HeaterConfig> {
        self.assets
            .iter()
            .find_map(|a| {
                if let AssetProfile::Heater(c) = a {
                    Some(c)
                } else {
                    None
                }
            })
            .or(self.devices.heater.as_ref())
    }

    /// Returns the PV config: checks `assets` list first, falls back to legacy `devices`.
    pub fn pv_config(&self) -> Option<&PvConfig> {
        self.assets
            .iter()
            .find_map(|a| {
                if let AssetProfile::Pv(c) = a {
                    Some(c)
                } else {
                    None
                }
            })
            .or(self.devices.pv.as_ref())
    }

    /// Returns the Battery config: checks `assets` list first, falls back to legacy `devices`.
    pub fn battery_config(&self) -> Option<&BatteryConfig> {
        self.assets
            .iter()
            .find_map(|a| {
                if let AssetProfile::Battery(c) = a {
                    Some(c)
                } else {
                    None
                }
            })
            .or(self.devices.battery.as_ref())
    }

    /// Returns the base load in kW: checks `assets` list first, falls back to legacy `devices.base_load_w`.
    pub fn base_load_kw(&self) -> f64 {
        self.assets
            .iter()
            .find_map(|a| {
                if let AssetProfile::BaseLoad(c) = a {
                    Some(c.baseline_kw)
                } else {
                    None
                }
            })
            .unwrap_or(self.devices.base_load_w / 1000.0)
    }

    pub async fn load(path: &str) -> Self {
        match Self::try_load(path).await {
            Ok(p) => {
                info!(path, "loaded simulator profile");
                p
            }
            Err(e) => {
                warn!(path, error = %e, "failed to load profile, using defaults");
                Self::default()
            }
        }
    }

    async fn try_load(path: &str) -> anyhow::Result<Self> {
        let contents = tokio::fs::read_to_string(Path::new(path)).await?;
        let profile: Profile = serde_yaml::from_str(&contents)?;
        Ok(profile)
    }

    pub fn default() -> Self {
        Self {
            devices: DeviceConfig::default(),
            assets: vec![],
            reactor: ReactorConfig::default(),
            simulator: SimulatorConfig::default(),
            planner: PlannerConfig::default(),
            packets: vec![],
            flexibility_policy: FlexibilityPolicy::default(),
        }
    }
}
