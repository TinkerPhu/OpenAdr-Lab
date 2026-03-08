use serde::Deserialize;
use std::path::Path;
use tracing::{info, warn};

#[derive(Debug, Clone, Deserialize)]
pub struct Profile {
    pub devices: DeviceConfig,
    #[serde(default)]
    pub reactor: ReactorConfig,
    #[serde(default)]
    pub simulator: SimulatorConfig,
}

#[derive(Debug, Clone, Deserialize)]
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
    #[serde(default = "default_ev_max_charge")]
    pub max_charge_kw: f64,
    #[serde(default = "default_ev_soc")]
    pub initial_soc: f64,
    #[serde(default = "default_ev_battery")]
    pub battery_kwh: f64,
    #[serde(default = "default_ev_soc_target")]
    pub soc_target: f64,
}

fn default_ev_max_charge() -> f64 {
    7.4
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
    #[serde(default = "default_heater_max")]
    pub max_kw: f64,
    #[serde(default = "default_heater_temp")]
    pub temp_initial_c: f64,
    #[serde(default = "default_heater_min")]
    pub temp_min_c: f64,
    #[serde(default = "default_heater_max_temp")]
    pub temp_max_c: f64,
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
    #[serde(default = "default_pv_rated")]
    pub rated_kw: f64,
}

fn default_pv_rated() -> f64 {
    5.0
}

#[derive(Debug, Clone, Deserialize)]
pub struct BatteryConfig {
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

impl Profile {
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
            devices: DeviceConfig {
                ev: None,
                heater: None,
                pv: None,
                battery: None,
                base_load_w: default_base_load(),
            },
            reactor: ReactorConfig::default(),
            simulator: SimulatorConfig::default(),
        }
    }
}
