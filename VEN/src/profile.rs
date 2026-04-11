use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::path::Path;
use tracing::{info, warn};

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
    pub simulator: SimulatorConfig,
    #[serde(default)]
    pub planner: PlannerConfig,
    #[serde(default)]
    pub grid: GridConfig,
    #[serde(default)]
    pub packets: Vec<PacketSeed>,
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
    /// Minimum charge power when plugged in (kW). EVSE semi-continuous lower bound:
    /// if charging at all, power must be at least this value (no trickle charging).
    /// Typical EVSE minimum: 6 A × 230 V ≈ 1.4 kW.
    #[serde(default = "default_ev_min_charge")]
    pub min_charge_kw: f64,
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
fn default_ev_min_charge() -> f64 {
    1.4
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
    /// Mid-power level (kW). Used by the MILP to model a two-level heater (mid / full).
    /// If absent, defaults to `max_kw / 2.0` at solve time.
    /// Set `mid_kw = max_kw` to model an on/off heater with a single power level.
    #[serde(default)]
    pub mid_kw: Option<f64>,
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

impl PvConfig {
    /// Forecast PV generation at a future time slot (kW, positive = generation).
    ///
    /// Uses the sin model: `rated_kw × sin(π × (hour − 6) / 12)` for hours 6–18,
    /// 0 otherwise. This is the same physics model used by `PvInverter::capability_trajectory`.
    /// When a real PV forecast API is integrated, only this method needs to change.
    pub fn forecast_kw(&self, ts: DateTime<Utc>) -> f64 {
        use chrono::Timelike;
        let hour = ts.hour() as f64 + ts.minute() as f64 / 60.0;
        if hour >= 6.0 && hour <= 18.0 {
            let angle = std::f64::consts::PI * (hour - 6.0) / 12.0;
            (angle.sin().max(0.0) * self.rated_kw).max(0.0)
        } else {
            0.0
        }
    }
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

/// Optimization objective preset. Selects a named weight configuration for the MILP solver.
/// Individual weight fields in `PlannerConfig` can be tuned further with `Custom`.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PlannerObjective {
    /// Minimize energy bill. Balanced weights: energy cost + light GHG + light grid + wear.
    /// (w_energy=1, w_ghg=0.20, w_grid=0.02, c_bat_wear=0.03)
    #[default]
    MinCost,
    /// Minimize carbon emissions above all else.
    /// (w_energy=0, w_ghg=10, w_grid=0, c_bat_wear=0)
    MinGhg,
    /// Minimize grid exchange volume (maximize self-consumption).
    /// (w_energy=0, w_ghg=0, w_grid=1, c_bat_wear=0)
    MinGrid,
    /// Maximize revenue from export and grid services.
    /// (w_energy=1, w_ghg=0, w_grid=0, c_bat_wear=0.03)
    MaxRevenue,
    /// Use the individual weight fields below directly, without any preset override.
    Custom,
}

/// Physical grid connection limits — meter / main breaker hard ceiling.
/// The MILP uses these as `p_imp_max_phys_kw` / `p_exp_max_phys_kw`.
/// When no OpenADR capacity event is active these also act as the contractual limit.
#[derive(Debug, Clone, Deserialize)]
pub struct GridConfig {
    /// Physical import limit at the meter or main breaker (kW).
    /// Default: 25.0 kW — typical residential 3-phase 32 A supply.
    #[serde(default = "default_max_import_kw")]
    pub max_import_kw: f64,
    /// Physical export limit (inverter / grid-tie maximum) (kW).
    /// Default: 10.0 kW.
    #[serde(default = "default_max_export_kw")]
    pub max_export_kw: f64,
}

fn default_max_import_kw() -> f64 {
    25.0
}
fn default_max_export_kw() -> f64 {
    10.0
}

impl Default for GridConfig {
    fn default() -> Self {
        Self {
            max_import_kw: default_max_import_kw(),
            max_export_kw: default_max_export_kw(),
        }
    }
}

/// Configuration for the HEMS Planner (Stage 3).
#[derive(Debug, Clone, Deserialize)]
pub struct PlannerConfig {
    /// Planning timestep in seconds (default 300 = 5 min).
    #[serde(default = "default_plan_step")]
    pub plan_step_s: u64,
    /// Total planning horizon in hours (default 24).
    #[serde(default = "default_plan_horizon_h")]
    pub plan_horizon_h: u64,
    /// Seconds between periodic replanning cycles (default 300).
    #[serde(default = "default_replan_interval")]
    pub replan_interval_s: u64,

    /// Scales the energy cost term (import tariff cost − export revenue).
    /// 1.0 = full economic optimization. 0.0 = ignore energy cost (e.g. pure GHG mode).
    #[serde(default = "default_w_energy")]
    pub w_energy: f64,
    /// Weight on GHG emissions: equivalent €/kgCO₂ added to objective.
    /// 0.0001 ≈ €100/tonne CO₂ — a light carbon price signal.
    #[serde(default = "default_w_ghg")]
    pub w_ghg: f64,
    /// Penalty per kWh of total grid exchange (import + export), in €/kWh.
    /// Drives the optimizer toward self-consumption. Default: 0.0 (disabled).
    #[serde(default)]
    pub w_grid: f64,
    /// Battery cycling wear cost in €/kWh charged or discharged.
    /// Prevents excessive cycling when arbitrage margin is thin.
    #[serde(default = "default_bat_wear")]
    pub c_bat_wear_eur_kwh: f64,
    /// Scales contractual limit violation penalties. 1.0 = normal; 0.0 = disabled.
    #[serde(default = "default_w_viol")]
    pub w_viol: f64,
    /// Per-kWh penalty for exceeding the contractual import limit (€/kWh slack).
    /// Default: 0.0 — see Penalty Modeling in the transition plan doc.
    #[serde(default)]
    pub pen_imp_eur_kwh: f64,
    /// Per-kWh penalty for exceeding the contractual export limit (€/kWh slack).
    /// Default: 0.0 — disabled.
    #[serde(default)]
    pub pen_exp_eur_kwh: f64,
    /// Reward per kWh of EV charging above the core energy requirement (€/kWh).
    /// Incentivises opportunistic top-up charging when tariffs are low.
    #[serde(default = "default_v_ev_extra")]
    pub v_ev_extra_eur_kwh: f64,
    /// Comfort reward for meeting the heater energy deadline (€, MayRun mode only).
    /// Acts as a comfort preference knob: higher → heat regardless of tariff.
    /// At typical 3 kW heating the threshold is ~€1.50 / session — a reasonable default.
    #[serde(default = "default_v_heat")]
    pub v_heat_eur: f64,
    /// Optimization objective preset. Selects weight ratios for the MILP solver.
    /// Set to `custom` to use the individual weight fields above directly.
    #[serde(default)]
    pub objective: PlannerObjective,
}

impl Default for PlannerConfig {
    fn default() -> Self {
        Self {
            plan_step_s: default_plan_step(),
            plan_horizon_h: default_plan_horizon_h(),
            replan_interval_s: default_replan_interval(),
            w_energy: default_w_energy(),
            w_ghg: default_w_ghg(),
            w_grid: 0.0,
            c_bat_wear_eur_kwh: default_bat_wear(),
            w_viol: default_w_viol(),
            pen_imp_eur_kwh: 0.0,
            pen_exp_eur_kwh: 0.0,
            v_ev_extra_eur_kwh: default_v_ev_extra(),
            v_heat_eur: default_v_heat(),
            objective: PlannerObjective::MinCost,
        }
    }
}

fn default_plan_step() -> u64 {
    300
}
fn default_plan_horizon_h() -> u64 {
    24
}
fn default_replan_interval() -> u64 {
    300
}
fn default_w_energy() -> f64 {
    1.0
}
fn default_w_ghg() -> f64 {
    0.0001
}
fn default_bat_wear() -> f64 {
    0.03
}
fn default_w_viol() -> f64 {
    1.0
}
fn default_v_ev_extra() -> f64 {
    0.10
}
fn default_v_heat() -> f64 {
    1.50
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
            simulator: SimulatorConfig::default(),
            planner: PlannerConfig::default(),
            grid: GridConfig::default(),
            packets: vec![],
        }
    }
}
