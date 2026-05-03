use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tracing::{info, warn};

/// Multi-asset deviation absorber configuration.
/// Tier 1 real-time controller for transient grid deviations.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct AbsorberConfig {
    /// Enable/disable absorber globally (default: false).
    pub enabled: bool,
    /// Magnitude threshold below which deviation is ignored, prevents chatter (default: 0.1 kW).
    pub dead_band_kw: f64,
    /// Ticks within dead-band before settling begins (default: 1).
    pub dead_band_clearing_ticks: usize,
    /// List of absorber-eligible assets with per-asset settings.
    pub assets: Vec<AbsorberAssetConfig>,
}

impl Default for AbsorberConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            dead_band_kw: 0.1,
            dead_band_clearing_ticks: 1,
            assets: Vec::new(),
        }
    }
}

/// Per-asset configuration for the absorber.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AbsorberAssetConfig {
    /// Asset ID — must match an asset in SimState.assets.
    pub id: String,
    /// Priority order (0 = first/battery, higher = later); should be unique.
    pub priority: u8,
    /// Minimum seconds between state changes (0 for electronics, 30–60 for relays).
    #[serde(default)]
    pub min_state_linger_s: u64,
    /// (EV only) Refuse charging reduction if departure < N seconds away.
    /// If unset (None), no guard applies. Typical: 1800 (30 minutes).
    #[serde(default)]
    pub ev_departure_guard_s: Option<u64>,
}

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
    /// Multi-asset deviation absorber configuration (Tier 1).
    #[serde(default)]
    pub absorber: AbsorberConfig,
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
    crate::ids::ASSET_EV.to_string()
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
}

impl HeaterConfig {
    /// Effective thermal mass (kWh/°C).
    /// Priority: `volume_l` → `thermal_mass_kwh_per_c` → 2.0 (legacy default).
    pub fn effective_thermal_mass(&self) -> f64 {
        if let Some(v) = self.volume_l {
            v * 4.186 / 3600.0
        } else {
            self.thermal_mass_kwh_per_c.unwrap_or(2.0)
        }
    }

    /// Effective Newton cooling coefficient (kW/°C).
    pub fn effective_k_loss(&self) -> f64 {
        self.k_loss_kw_per_c.unwrap_or(0.1)
    }

    /// Effective constant hot water draw (kW thermal).
    pub fn effective_draw_kw(&self) -> f64 {
        self.draw_kw.unwrap_or(0.0)
    }

    /// Relay switching penalty coefficient [EUR/switch event] for the MILP objective.
    pub fn effective_switching_penalty(&self) -> f64 {
        self.switching_penalty_eur.unwrap_or(0.01)
    }
}

fn default_asset_id_heater() -> String {
    crate::ids::ASSET_HEATER.to_string()
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
    crate::ids::ASSET_PV.to_string()
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
    crate::ids::ASSET_BATTERY.to_string()
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
    crate::ids::ASSET_BASE_LOAD.to_string()
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PlannerObjective {
    /// Minimize energy bill. Balanced weights: energy cost + light GHG + light grid + wear.
    /// (w_energy=1, w_ghg=0.20, w_grid=0.02, c_bat_wear=0.03)
    #[default]
    MinCost,
    /// Minimize carbon emissions above all else.
    /// (w_energy=0, w_ghg=10, w_grid=0, c_bat_wear=0)
    MinGhg,
    /// Minimize grid exchange volume (import + export equally penalised).
    /// (w_energy=0, w_ghg=0, w_grid=1, c_bat_wear=0)
    MinGrid,
    /// Minimize grid import only — export is allowed/encouraged (energy autarky).
    /// (w_import=1, w_energy=0, w_ghg=0, w_grid=0, c_bat_wear=0)
    MinImport,
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

    /// Minimum absolute grid import error (kW) that activates battery correction.
    /// Layer 1 fires when |actual_net_kw − planned_net_kw| exceeds this value.
    /// Set to 0.0 to disable Layer 1 correction entirely. Default: 1.0 kW.
    #[serde(default = "default_deviation_threshold_kw")]
    pub deviation_threshold_kw: f64,

    /// Consecutive 1-second ticks of sustained deviation before a DeviceDeviation
    /// replan is triggered (Layer 2). Default: 30 (= 30 seconds).
    #[serde(default = "default_deviation_trigger_ticks")]
    pub deviation_trigger_ticks: u32,

    /// Minimum battery setpoint change to apply (noise floor). Corrections smaller
    /// than this are suppressed to avoid chattering. Default: 0.2 kW.
    #[serde(default = "default_correction_min_kw")]
    pub correction_min_kw: f64,

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
    /// Startup penalty per EV charging run [€/run].
    /// Breaks degeneracy: encourages one contiguous charging block rather than fragmented slots.
    #[serde(default = "default_ev_startup")]
    pub c_ev_startup_eur: f64,
    /// Startup penalty per battery charge/discharge mode transition [€/transition].
    /// Encourages contiguous charge and discharge blocks rather than scattered spikes.
    #[serde(default = "default_bat_startup")]
    pub c_bat_startup_eur: f64,
    /// Ramp penalty per kW of EV power change between consecutive slots [€/kW].
    /// Penalises |p_ev[t] - p_ev[t-1]|; keeps charging at a stable power level.
    #[serde(default = "default_ev_ramp")]
    pub c_ev_ramp_eur_kw: f64,
    /// Ramp penalty per kW of battery net-power change between consecutive slots [€/kW].
    /// Penalises |(p_ch[t]−p_dis[t]) − (p_ch[t−1]−p_dis[t−1])|; smooths battery power.
    #[serde(default = "default_bat_ramp")]
    pub c_bat_ramp_eur_kw: f64,
    /// Penalty per kWh of battery discharge co-occurring with EV charging in slots where
    /// PV surplus (p_pv − p_base) ≥ p_ev_min_kw [€/kWh]. Discourages unnecessary battery
    /// cycling when free PV power is available to cover the EV load.
    /// Set to 0.0 to disable. Default: 0.5.
    #[serde(default = "default_bat_ev_coexist")]
    pub c_bat_ev_coexist_eur_kwh: f64,
    /// Scales contractual limit violation penalties. 1.0 = normal; 0.0 = disabled.
    #[serde(default = "default_w_viol")]
    pub w_viol: f64,
    /// Per-kWh penalty for exceeding the contractual import limit (€/kWh slack).
    /// Default: 10 000 — high enough that no realistic energy saving outweighs slack cost.
    #[serde(default = "default_pen_imp")]
    pub pen_imp_eur_kwh: f64,
    /// Per-kWh penalty for exceeding the contractual export limit (€/kWh slack).
    /// Default: 10 000 — symmetric with import penalty.
    #[serde(default = "default_pen_exp")]
    pub pen_exp_eur_kwh: f64,
    /// Reward per kWh of EV charging above the core energy requirement (€/kWh).
    /// Incentivises opportunistic top-up charging when tariffs are low.
    #[serde(default = "default_v_ev_extra")]
    pub v_ev_extra_eur_kwh: f64,
    /// Soft penalty per slot for using the heater's full power tier over mid tier [€/slot].
    /// Breaks ties in favour of mid tier (e.g. 3 kW) over full tier (e.g. 6 kW) when tariff
    /// savings are equal. Must be small relative to actual energy cost differences.
    /// Default: 0.001 EUR/slot.
    #[serde(default = "default_w_tier_penalty")]
    pub w_tier_penalty_eur: f64,
    /// Optimization objective preset. Selects weight ratios for the MILP solver.
    /// Set to `custom` to use the individual weight fields above directly.
    #[serde(default)]
    pub objective: PlannerObjective,

    /// Minimum objective improvement (EUR) required to replace the current plan on a
    /// Periodic replan trigger. Hard triggers (RateChange, CapacityChange, Alert,
    /// UserRequest, DeviceDeviation, AssetStateChange) always force adoption.
    /// 0.0 = always adopt (default — unchanged behaviour). Raise to e.g. 0.20 to
    /// suppress plan churn when the new solution is only marginally better.
    #[serde(default)]
    pub plan_adoption_threshold_eur: f64,

    /// Time constant (seconds) for linear decay of `plan_adoption_threshold_eur`.
    /// As time flows the rolling planning window shifts, so a new plan cannot always
    /// beat the old one in absolute EUR even when it is genuinely optimal for current
    /// conditions. The effective threshold at the adoption gate is:
    ///   effective = threshold × max(0, 1 − elapsed_s / decay_s)
    /// After `decay_s` seconds the effective threshold reaches 0.0 and any new plan
    /// is accepted. 0.0 = no decay (default — full threshold always applied).
    /// Suggested: 5–10× `replan_interval_s`.
    #[serde(default)]
    pub plan_adoption_decay_s: f64,
    /// Cost cap slack for Phase 2 lexicographic solve [EUR]. Phase 2 minimises
    /// operational friction (startup/ramp/switching/tier) subject to:
    ///   phase1_cost ≤ c_star + phase2_epsilon_eur
    /// Set to 0.0 to disable Phase 2 (single-phase solve). Default: 0.02.
    #[serde(default = "default_phase2_epsilon")]
    pub phase2_epsilon_eur: f64,
}

impl Default for PlannerConfig {
    fn default() -> Self {
        Self {
            plan_step_s: default_plan_step(),
            plan_horizon_h: default_plan_horizon_h(),
            replan_interval_s: default_replan_interval(),
            deviation_threshold_kw: default_deviation_threshold_kw(),
            deviation_trigger_ticks: default_deviation_trigger_ticks(),
            correction_min_kw: default_correction_min_kw(),
            w_energy: default_w_energy(),
            w_ghg: default_w_ghg(),
            w_grid: 0.0,
            c_bat_wear_eur_kwh: default_bat_wear(),
            c_ev_startup_eur: default_ev_startup(),
            c_bat_startup_eur: default_bat_startup(),
            c_ev_ramp_eur_kw: default_ev_ramp(),
            c_bat_ramp_eur_kw: default_bat_ramp(),
            c_bat_ev_coexist_eur_kwh: default_bat_ev_coexist(),
            w_viol: default_w_viol(),
            pen_imp_eur_kwh: default_pen_imp(),
            pen_exp_eur_kwh: default_pen_exp(),
            v_ev_extra_eur_kwh: default_v_ev_extra(),
            w_tier_penalty_eur: default_w_tier_penalty(),
            objective: PlannerObjective::MinCost,
            plan_adoption_threshold_eur: 0.0,
            plan_adoption_decay_s: 0.0,
            phase2_epsilon_eur: default_phase2_epsilon(),
        }
    }
}

fn default_phase2_epsilon() -> f64 {
    0.02
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
fn default_deviation_threshold_kw() -> f64 {
    1.0
}
fn default_deviation_trigger_ticks() -> u32 {
    30
}
fn default_correction_min_kw() -> f64 {
    0.2
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
fn default_ev_startup() -> f64 {
    0.01
}
fn default_bat_startup() -> f64 {
    0.01
}
fn default_ev_ramp() -> f64 {
    0.005
}
fn default_bat_ramp() -> f64 {
    0.005
}
fn default_bat_ev_coexist() -> f64 {
    0.5
}
fn default_w_viol() -> f64 {
    1.0
}
fn default_v_ev_extra() -> f64 {
    0.10
}
fn default_w_tier_penalty() -> f64 {
    0.001
}
fn default_pen_imp() -> f64 {
    10_000.0
}
fn default_pen_exp() -> f64 {
    10_000.0
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
    /// Returns the EV config from the `assets` list.
    pub fn ev_config(&self) -> Option<&EvConfig> {
        self.assets.iter().find_map(|a| {
            if let AssetProfile::Ev(c) = a { Some(c) } else { None }
        })
    }

    /// Returns the Heater config from the `assets` list.
    pub fn heater_config(&self) -> Option<&HeaterConfig> {
        self.assets.iter().find_map(|a| {
            if let AssetProfile::Heater(c) = a { Some(c) } else { None }
        })
    }

    /// Returns the PV config from the `assets` list.
    pub fn pv_config(&self) -> Option<&PvConfig> {
        self.assets.iter().find_map(|a| {
            if let AssetProfile::Pv(c) = a { Some(c) } else { None }
        })
    }

    /// Returns the Battery config from the `assets` list.
    pub fn battery_config(&self) -> Option<&BatteryConfig> {
        self.assets.iter().find_map(|a| {
            if let AssetProfile::Battery(c) = a { Some(c) } else { None }
        })
    }

    /// Returns the base load in kW from the `assets` list.
    pub fn base_load_kw(&self) -> f64 {
        self.assets
            .iter()
            .find_map(|a| {
                if let AssetProfile::BaseLoad(c) = a { Some(c.baseline_kw) } else { None }
            })
            .unwrap_or_else(default_base_load_kw)
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

    pub async fn try_load(path: &str) -> anyhow::Result<Self> {
        let contents = tokio::fs::read_to_string(Path::new(path)).await?;
        let profile: Profile = serde_yaml::from_str(&contents)?;
        if profile.assets.is_empty() {
            anyhow::bail!("profile at '{}' has no assets — check the YAML 'assets:' list", path);
        }
        Ok(profile)
    }

    pub fn default() -> Self {
        Self {
            assets: vec![],
            simulator: SimulatorConfig::default(),
            planner: PlannerConfig::default(),
            grid: GridConfig::default(),
            packets: vec![],
            absorber: AbsorberConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heater_config_switching_penalty_default() {
        let cfg = HeaterConfig {
            id: "heater".into(),
            max_kw: 3.0,
            temp_initial_c: 20.0,
            temp_min_c: 18.0,
            temp_max_c: 23.0,
            mid_kw: None,
            volume_l: None,
            thermal_mass_kwh_per_c: None,
            k_loss_kw_per_c: None,
            draw_kw: None,
            switching_penalty_eur: None,
        };
        assert!((cfg.effective_switching_penalty() - 0.01).abs() < 1e-9);
    }

    #[test]
    fn heater_config_switching_penalty_explicit() {
        let cfg = HeaterConfig {
            id: "heater".into(),
            max_kw: 3.0,
            temp_initial_c: 20.0,
            temp_min_c: 18.0,
            temp_max_c: 23.0,
            mid_kw: None,
            volume_l: None,
            thermal_mass_kwh_per_c: None,
            k_loss_kw_per_c: None,
            draw_kw: None,
            switching_penalty_eur: Some(0.05),
        };
        assert!((cfg.effective_switching_penalty() - 0.05).abs() < 1e-9);
    }

    #[test]
    fn heater_config_yaml_without_penalty_field() {
        let yaml = r#"
type: heater
id: heater
max_kw: 3.0
temp_initial_c: 20.0
temp_min_c: 18.0
temp_max_c: 23.0
"#;
        let asset: AssetProfile = serde_yaml::from_str(yaml).expect("should parse heater yaml");
        if let AssetProfile::Heater(cfg) = asset {
            assert!(cfg.switching_penalty_eur.is_none(), "penalty should default to None");
            assert!((cfg.effective_switching_penalty() - 0.01).abs() < 1e-9);
        } else {
            panic!("expected AssetProfile::Heater");
        }
    }

    #[tokio::test]
    async fn profile_empty_assets_guard() {
        // try_load must reject a YAML that parses but has no assets.
        let dir = std::env::temp_dir();
        let path = dir.join("empty_assets_profile_test.yaml");
        tokio::fs::write(&path, "simulator:\n  tick_s: 1\n").await.unwrap();
        let result = Profile::try_load(path.to_str().unwrap()).await;
        assert!(result.is_err(), "try_load must return Err for empty assets list");
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("no assets"), "error message should mention 'no assets': {msg}");
        let _ = tokio::fs::remove_file(path).await;
    }

    #[test]
    fn absorber_config_with_all_fields() {
        let yaml = r#"
absorber:
  enabled: true
  dead_band_kw: 0.1
  dead_band_clearing_ticks: 1
  assets:
    - id: battery
      priority: 0
      min_state_linger_s: 0
    - id: ev
      priority: 1
      min_state_linger_s: 0
      ev_departure_guard_s: 1800
    - id: heater
      priority: 2
      min_state_linger_s: 30
"#;
        let profile: Profile = serde_yaml::from_str(yaml).expect("should parse absorber yaml");
        assert!(profile.absorber.enabled, "enabled should be true");
        assert!((profile.absorber.dead_band_kw - 0.1).abs() < 1e-9, "dead_band_kw should be 0.1");
        assert_eq!(profile.absorber.dead_band_clearing_ticks, 1, "dead_band_clearing_ticks should be 1");
        assert_eq!(profile.absorber.assets.len(), 3, "should have 3 assets");
        assert_eq!(profile.absorber.assets[0].id, "battery");
        assert_eq!(profile.absorber.assets[0].priority, 0);
        assert_eq!(profile.absorber.assets[1].id, "ev");
        assert_eq!(profile.absorber.assets[1].ev_departure_guard_s, Some(1800));
        assert_eq!(profile.absorber.assets[2].id, "heater");
        assert_eq!(profile.absorber.assets[2].min_state_linger_s, 30);
    }

    #[test]
    fn absorber_config_without_section_defaults_to_disabled() {
        let yaml = r#"
assets:
  - type: battery
    id: battery
    battery_kwh: 60
"#;
        let profile: Profile = serde_yaml::from_str(yaml).expect("should parse yaml without absorber");
        assert!(!profile.absorber.enabled, "enabled should default to false");
        assert!((profile.absorber.dead_band_kw - 0.1).abs() < 1e-9, "dead_band_kw should default to 0.1");
        assert_eq!(profile.absorber.assets.len(), 0, "absorber assets should be empty");
    }

    #[test]
    fn absorber_asset_config_defaults_when_fields_omitted() {
        let yaml = r#"
absorber:
  enabled: true
  assets:
    - id: battery
      priority: 0
"#;
        let profile: Profile = serde_yaml::from_str(yaml).expect("should parse with partial fields");
        assert_eq!(profile.absorber.assets.len(), 1);
        assert_eq!(profile.absorber.assets[0].id, "battery");
        assert_eq!(profile.absorber.assets[0].priority, 0);
        assert_eq!(profile.absorber.assets[0].min_state_linger_s, 0, "min_state_linger_s should default to 0");
        assert!(profile.absorber.assets[0].ev_departure_guard_s.is_none(), "ev_departure_guard_s should default to None");
    }

    #[test]
    fn absorber_config_dead_band_clearing_ticks_default() {
        let yaml = r#"
absorber:
  enabled: true
  assets: []
"#;
        let profile: Profile = serde_yaml::from_str(yaml).expect("should parse without dead_band_clearing_ticks");
        assert_eq!(profile.absorber.dead_band_clearing_ticks, 1, "dead_band_clearing_ticks should default to 1");
    }
}
