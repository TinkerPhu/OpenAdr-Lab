use crate::entities::asset_params::{
    AssetParams, BaseLoadParams, BatteryParams, EvParams, HeaterParams, PvParams,
};
use crate::entities::plan::PlanZone;
use crate::entities::PlannerObjective;
use serde::Deserialize;
use std::path::Path;

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
            }),
            AssetProfile::Heater(c) => AssetParams::Heater(HeaterParams {
                id: c.id.clone(),
                max_kw: c.max_kw,
                temp_initial_c: c.temp_initial_c,
                temp_min_c: c.temp_min_c,
                temp_max_c: c.temp_max_c,
                mid_kw: c.mid_kw,
                thermal_mass_kwh_per_c: c.effective_thermal_mass(),
                k_loss_kw_per_c: c.effective_k_loss(),
                draw_kw: c.effective_draw_kw(),
                switching_penalty_eur: c.effective_switching_penalty(),
                c_terminal_eur_kwh: c.c_terminal_eur_kwh,
            }),
            AssetProfile::Pv(c) => AssetParams::Pv(PvParams {
                id: c.id.clone(),
                rated_kw: c.rated_kw,
            }),
            AssetProfile::BaseLoad(c) => AssetParams::BaseLoad(BaseLoadParams {
                id: c.id.clone(),
                baseline_kw: c.baseline_kw,
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
    /// Override for auto-computed terminal energy reward [EUR/kWh].
    /// Omit (or None) → auto-compute from mean(c_imp) + c_ctrl_imp_malus.
    /// 0.0 → disabled. Any positive value → fixed coefficient.
    #[serde(default)]
    pub c_terminal_eur_kwh: Option<f64>,
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
    /// Optional override for auto-computed terminal energy reward [EUR/kWh].
    /// None (omitted in YAML): auto-compute from avg import tariff × round_trip_efficiency.
    /// Some(0.0): disabled. Some(x): fixed at x EUR/kWh.
    #[serde(default)]
    pub c_terminal_eur_kwh: Option<f64>,
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

#[derive(Debug, Clone, Deserialize)]
pub struct PlannerConfig {
    /// Optional variable-step planning grid. When set, `plan_step_s` and `plan_horizon_h`
    /// are ignored and the effective values are derived from the zones list.
    /// Production profiles omit this field; the uniform-step defaults apply.
    /// Test profiles can use a single coarse zone for fast solver runs.
    #[serde(default)]
    pub plan_zones: Option<Vec<PlanZone>>,
    /// Planning timestep in seconds (default 600 = 10 min). Ignored when `plan_zones` is set.
    #[serde(default = "default_plan_step")]
    pub plan_step_s: u64,
    /// Total planning horizon in hours (default 48). Ignored when `plan_zones` is set.
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
    /// One-time reward (EUR) per kWh of core energy target for committing to a
    /// soft-deadline EV session (MayRun mode). Must exceed the expected charging
    /// cost for the optimizer to choose z_ev_core = 1. Default: 1.0 EUR/kWh
    /// (~3–5× typical peak tariff), overridable per-VEN in profile YAML.
    #[serde(default = "default_v_ev_core")]
    pub v_ev_core_eur_kwh: f64,
    /// Soft penalty per slot for using the heater's full power tier over mid tier [€/slot].
    /// Breaks ties in favour of mid tier (e.g. 3 kW) over full tier (e.g. 6 kW) when tariff
    /// savings are equal. Must be small relative to actual energy cost differences.
    /// Default: 0.001 EUR/slot.
    #[serde(default = "default_w_tier_penalty")]
    pub w_tier_penalty_eur: f64,
    /// Phase 1 penalty [€/kWh] on controllable-asset import exceeding free PV surplus.
    /// Covers all controllable assets as a group (heater + EV + net battery + shiftables).
    /// When the total controllable load exceeds `max(0, p_pv − p_base)` the excess kWh
    /// is penalised at this rate, discouraging pre-storage arbitrage beyond what PV provides.
    /// Set to ~0.20–0.25 to prefer mid-tier when PV exactly covers it.
    /// Default: 0.0 (disabled — existing behaviour preserved).
    #[serde(default = "default_c_ctrl_imp_malus")]
    pub c_ctrl_imp_malus_eur_kwh: f64,
    /// Optimization objective preset. Selects weight ratios for the MILP solver.
    /// Set to `custom` to use the individual weight fields above directly.
    #[serde(default)]
    pub objective: PlannerObjective,

    /// Minimum objective improvement (EUR) required to replace the current plan on a
    /// Periodic replan trigger. Hard triggers (RateChange, CapacityChange, Alert,
    /// UserRequest, AssetStateChange) always force adoption.
    /// 0.0 = always adopt. Default: 0.20 (suppress churn when improvement is marginal).
    #[serde(default = "default_plan_adoption_threshold")]
    pub plan_adoption_threshold_eur: f64,

    /// Time constant (seconds) for linear decay of `plan_adoption_threshold_eur`.
    /// As time flows the rolling planning window shifts, so a new plan cannot always
    /// beat the old one in absolute EUR even when it is genuinely optimal for current
    /// conditions. The effective threshold at the adoption gate is:
    ///   effective = threshold × max(0, 1 − elapsed_s / decay_s)
    /// After `decay_s` seconds the effective threshold reaches 0.0 and any new plan
    /// is accepted. 0.0 = no decay (full threshold always applied).
    /// Default: 1500 s (5× replan_interval_s).
    #[serde(default = "default_plan_adoption_decay")]
    pub plan_adoption_decay_s: f64,
    /// Cost cap slack for Phase 2 lexicographic solve [EUR]. Phase 2 minimises
    /// operational friction (startup/ramp/switching/tier) subject to:
    ///   phase1_cost ≤ c_star + phase2_epsilon_eur
    /// Set to 0.0 to disable Phase 2 (single-phase solve). Default: 0.02.
    #[serde(default = "default_phase2_epsilon")]
    pub phase2_epsilon_eur: f64,

    /// HiGHS solver time limit per phase in seconds. Default: 60.
    #[serde(default = "default_solver_timeout_s")]
    pub solver_timeout_s: u64,

    /// Seconds the planning loop sleeps after startup before the first plan.
    /// Allows event polling to populate tariff rates first. Default: 5.
    #[serde(default = "default_planning_initial_delay_s")]
    pub planning_initial_delay_s: u64,

    /// Per-extra-switch surcharge [EUR] added to the effective acceptance threshold.
    /// Periodic replans that introduce more heater relay operations than the current plan
    /// must overcome this additional cost penalty before being adopted.
    /// 0.0 = disabled (default). Suggested: match `switching_penalty_eur`.
    #[serde(default)]
    pub gate_switch_penalty_eur: f64,
}

impl PlannerConfig {
    /// Effective slot width: zone[0].step_s when plan_zones is set, else plan_step_s.
    pub fn effective_step_s(&self) -> u64 {
        self.plan_zones
            .as_ref()
            .and_then(|z| z.first())
            .map(|z| z.step_s)
            .unwrap_or(self.plan_step_s)
    }

    /// Effective horizon: sum(step_s × slots) / 3600 when plan_zones is set, else plan_horizon_h.
    pub fn effective_horizon_h(&self) -> u64 {
        self.plan_zones
            .as_ref()
            .filter(|z| !z.is_empty())
            .map(|zones| zones.iter().map(|z| z.step_s * z.slots as u64).sum::<u64>() / 3600)
            .unwrap_or(self.plan_horizon_h)
    }
}

impl Default for PlannerConfig {
    fn default() -> Self {
        Self {
            plan_zones: None,
            plan_step_s: default_plan_step(),
            plan_horizon_h: default_plan_horizon_h(),
            replan_interval_s: default_replan_interval(),
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
            v_ev_core_eur_kwh: default_v_ev_core(),
            w_tier_penalty_eur: default_w_tier_penalty(),
            c_ctrl_imp_malus_eur_kwh: default_c_ctrl_imp_malus(),
            objective: PlannerObjective::MinCost,
            plan_adoption_threshold_eur: default_plan_adoption_threshold(),
            plan_adoption_decay_s: default_plan_adoption_decay(),
            phase2_epsilon_eur: default_phase2_epsilon(),
            solver_timeout_s: default_solver_timeout_s(),
            planning_initial_delay_s: default_planning_initial_delay_s(),
            gate_switch_penalty_eur: 0.0,
        }
    }
}

fn default_phase2_epsilon() -> f64 {
    0.02
}
fn default_plan_adoption_threshold() -> f64 {
    0.20
}
fn default_plan_adoption_decay() -> f64 {
    1500.0
}
fn default_solver_timeout_s() -> u64 {
    60
}
fn default_planning_initial_delay_s() -> u64 {
    5
}

fn default_plan_step() -> u64 {
    600
}
fn default_plan_horizon_h() -> u64 {
    48
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
fn default_v_ev_core() -> f64 {
    1.0
}
fn default_w_tier_penalty() -> f64 {
    0.001
}
fn default_c_ctrl_imp_malus() -> f64 {
    0.22
}
fn default_pen_imp() -> f64 {
    10_000.0
}
fn default_pen_exp() -> f64 {
    10_000.0
}

impl Profile {
    pub async fn try_load(path: &str) -> anyhow::Result<Self> {
        let contents = tokio::fs::read_to_string(Path::new(path)).await?;
        let profile: Profile = serde_yaml::from_str(&contents)?;
        if profile.assets.is_empty() {
            anyhow::bail!(
                "profile at '{}' has no assets — check the YAML 'assets:' list",
                path
            );
        }
        Ok(profile)
    }

    /// Validate profile invariants. Returns all violations at once so the user
    /// can fix all problems in a single startup attempt.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors: Vec<String> = Vec::new();

        // At least one asset declared.
        if self.assets.is_empty() {
            errors.push("profile must declare at least one asset".into());
        }

        // Planner numeric bounds.
        if self.planner.replan_interval_s == 0 {
            errors.push("planner.replan_interval_s must be > 0".into());
        }
        if self.planner.phase2_epsilon_eur < 0.0 {
            errors.push(format!(
                "planner.phase2_epsilon_eur must be ≥ 0.0, got {}",
                self.planner.phase2_epsilon_eur
            ));
        }

        // Per-asset numeric bounds.
        for asset in &self.assets {
            match asset {
                AssetProfile::Ev(c) => {
                    if !(0.0..=1.0).contains(&c.soc_target) {
                        errors.push(format!(
                            "ev.soc_target must be in [0.0, 1.0], got {}",
                            c.soc_target
                        ));
                    }
                    if c.max_discharge_kw < 0.0 {
                        errors.push(format!(
                            "ev.max_discharge_kw must be ≥ 0.0, got {}",
                            c.max_discharge_kw
                        ));
                    }
                }
                AssetProfile::Battery(c) => {
                    if !(0.0..1.0).contains(&c.min_soc) {
                        errors.push(format!(
                            "battery.min_soc must be in [0.0, 1.0), got {}",
                            c.min_soc
                        ));
                    }
                    if c.round_trip_efficiency <= 0.0 || c.round_trip_efficiency > 1.0 {
                        errors.push(format!(
                            "battery.round_trip_efficiency must be in (0.0, 1.0], got {}",
                            c.round_trip_efficiency
                        ));
                    }
                }
                _ => {}
            }
        }

        // plan_zones constraints: every zone's step_s must be a multiple of zone[0].step_s;
        // no zone may have step_s == 0 or slots == 0.
        if let Some(zones) = &self.planner.plan_zones {
            let base = zones.first().map(|z| z.step_s).unwrap_or(0);
            if base == 0 {
                errors.push("plan_zones[0].step_s must be > 0".into());
            } else {
                for (i, z) in zones.iter().enumerate() {
                    if z.step_s == 0 {
                        errors.push(format!("plan_zones[{i}].step_s must be > 0"));
                    } else if z.step_s % base != 0 {
                        errors.push(format!(
                            "plan_zones[{i}].step_s ({}) is not a multiple of zone[0].step_s ({})",
                            z.step_s, base
                        ));
                    }
                    if z.slots == 0 {
                        errors.push(format!("plan_zones[{i}].slots must be > 0"));
                    }
                }
            }
        }

        // phase2_epsilon_eur sanity check: when a heater is present and the epsilon is
        // non-zero, it must not exceed 6× the effective per-switch cost
        // (switching_penalty_eur × step_s/3600). At 6× the effective cost the epsilon
        // already allows the Phase 2 solver to accept solutions with 6 extra relay
        // operations; values well above this override the Phase 1 cost objective.
        if self.planner.phase2_epsilon_eur > 0.0 {
            if let Some(AssetProfile::Heater(h)) = self
                .assets
                .iter()
                .find(|a| matches!(a, AssetProfile::Heater(_)))
            {
                // Use the longest zone step for the bound — that is the most expensive
                // switch in MILP terms, giving the most conservative (largest) ceiling.
                let longest_step_s =
                    self.planner
                        .plan_zones
                        .as_ref()
                        .and_then(|z| z.iter().map(|z| z.step_s).max())
                        .unwrap_or(self.planner.plan_step_s) as f64;
                let effective_switch_cost =
                    h.effective_switching_penalty() * (longest_step_s / 3600.0);
                let sanity_bound = effective_switch_cost * 6.0;
                if sanity_bound > 0.0 && self.planner.phase2_epsilon_eur > sanity_bound {
                    let ratio = self.planner.phase2_epsilon_eur / effective_switch_cost;
                    let target = effective_switch_cost * 2.0;
                    errors.push(format!(
                        "planner.phase2_epsilon_eur ({:.3}) is {:.1}× the effective per-switch \
                         cost ({:.3} EUR); expected ≤ {:.3}. Reduce to ~{:.2}.",
                        self.planner.phase2_epsilon_eur,
                        ratio,
                        effective_switch_cost,
                        sanity_bound,
                        target,
                    ));
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    pub fn default() -> Self {
        Self {
            assets: vec![],
            simulator: SimulatorConfig::default(),
            planner: PlannerConfig::default(),
            grid: GridConfig::default(),
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
            c_terminal_eur_kwh: None,
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
            c_terminal_eur_kwh: None,
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
            assert!(
                cfg.switching_penalty_eur.is_none(),
                "penalty should default to None"
            );
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
        tokio::fs::write(&path, "simulator:\n  tick_s: 1\n")
            .await
            .unwrap();
        let result = Profile::try_load(path.to_str().unwrap()).await;
        assert!(
            result.is_err(),
            "try_load must return Err for empty assets list"
        );
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("no assets"),
            "error message should mention 'no assets': {msg}"
        );
        let _ = tokio::fs::remove_file(path).await;
    }

    fn make_valid_profile() -> Profile {
        let yaml = r#"
assets:
  - type: battery
    id: battery
    capacity_kwh: 10.0
    min_soc: 0.10
    round_trip_efficiency: 0.92
  - type: ev
    id: ev
    soc_target: 0.80
    max_discharge_kw: 0.0
"#;
        serde_yaml::from_str(yaml).unwrap()
    }

    #[test]
    fn validate_passes_for_valid_profile() {
        let p = make_valid_profile();
        assert!(p.validate().is_ok(), "valid profile must pass validation");
    }

    #[test]
    fn validate_fails_for_empty_assets() {
        let mut p = make_valid_profile();
        p.assets.clear();
        let errs = p.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.contains("at least one asset")));
    }

    #[test]
    fn validate_fails_for_soc_target_out_of_range() {
        let yaml = r#"
assets:
  - type: ev
    id: ev
    soc_target: 1.5
"#;
        let p: Profile = serde_yaml::from_str(yaml).unwrap();
        let errs = p.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.contains("soc_target")));
    }

    #[test]
    fn validate_fails_for_round_trip_efficiency_zero() {
        let yaml = r#"
assets:
  - type: battery
    id: battery
    round_trip_efficiency: 0.0
"#;
        let p: Profile = serde_yaml::from_str(yaml).unwrap();
        let errs = p.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.contains("round_trip_efficiency")));
    }

    #[test]
    fn validate_reports_multiple_violations_at_once() {
        let yaml = r#"
assets:
  - type: ev
    id: ev
    soc_target: 1.5
    max_discharge_kw: -1.0
"#;
        let p: Profile = serde_yaml::from_str(yaml).unwrap();
        let errs = p.validate().unwrap_err();
        assert!(
            errs.len() >= 2,
            "expected ≥ 2 errors, got {}: {:?}",
            errs.len(),
            errs
        );
    }

    fn make_heater_profile(switching_penalty_eur: f64, phase2_epsilon_eur: f64) -> Profile {
        let yaml = format!(
            r#"
assets:
  - type: heater
    id: heater
    max_kw: 6.0
    temp_initial_c: 50.0
    temp_min_c: 45.0
    temp_max_c: 60.0
    switching_penalty_eur: {switching_penalty_eur}
planner:
  phase2_epsilon_eur: {phase2_epsilon_eur}
"#
        );
        serde_yaml::from_str(&yaml).unwrap()
    }

    #[test]
    fn test_validate_phase2_epsilon_rejects_misconfiguration() {
        // switching_penalty=0.50, step=600s → effective=0.083 EUR/switch, bound=0.50
        // phase2_epsilon=5.0 is ~10× the bound → must be rejected
        let p = make_heater_profile(0.50, 5.0);
        let errs = p.validate().unwrap_err();
        assert!(
            errs.iter().any(|e| e.contains("phase2_epsilon_eur")),
            "expected phase2_epsilon_eur violation, got: {errs:?}"
        );
    }

    #[test]
    fn test_validate_phase2_epsilon_accepts_correct_value() {
        // 2× effective cost = 2 × 0.083 ≈ 0.17 EUR → well within bound
        let p = make_heater_profile(0.50, 0.17);
        assert!(
            p.validate().is_ok(),
            "phase2_epsilon_eur=0.17 should pass validation"
        );
    }

    #[test]
    fn test_validate_phase2_epsilon_skipped_without_heater() {
        // No heater → check is irrelevant regardless of value
        let yaml = r#"
assets:
  - type: battery
    id: battery
    capacity_kwh: 10.0
    min_soc: 0.10
    round_trip_efficiency: 0.92
planner:
  phase2_epsilon_eur: 99.0
"#;
        let p: Profile = serde_yaml::from_str(yaml).unwrap();
        assert!(
            p.validate().is_ok(),
            "no heater → phase2_epsilon check must be skipped"
        );
    }

    #[test]
    fn test_default_planner_config_has_correct_values() {
        let cfg = PlannerConfig::default();
        assert_eq!(cfg.c_ctrl_imp_malus_eur_kwh, 0.22);
        assert_eq!(cfg.plan_adoption_threshold_eur, 0.20);
        assert!((cfg.plan_adoption_decay_s - 1500.0).abs() < 1e-9);
        assert_eq!(cfg.plan_step_s, 600);
        assert_eq!(cfg.plan_horizon_h, 48);
    }

    #[test]
    fn test_plan_zones_derive_effective_step_and_horizon() {
        let mut cfg = PlannerConfig::default();
        cfg.plan_zones = Some(vec![
            PlanZone {
                step_s: 300,
                slots: 96,
            }, // 8 h
            PlanZone {
                step_s: 600,
                slots: 96,
            }, // 16 h
            PlanZone {
                step_s: 900,
                slots: 96,
            }, // 24 h
        ]);
        assert_eq!(cfg.effective_step_s(), 300);
        assert_eq!(cfg.effective_horizon_h(), 48);
    }

    #[test]
    fn test_plan_zones_single_zone_matches_test_profile_values() {
        let mut cfg = PlannerConfig::default();
        cfg.plan_zones = Some(vec![PlanZone {
            step_s: 3600,
            slots: 24,
        }]);
        assert_eq!(cfg.effective_step_s(), 3600);
        assert_eq!(cfg.effective_horizon_h(), 24);
    }

    #[test]
    fn test_plan_zones_no_zones_falls_back_to_scalar() {
        let cfg = PlannerConfig::default();
        // plan_zones absent → effective values come from plan_step_s / plan_horizon_h defaults
        assert_eq!(cfg.effective_step_s(), 600);
        assert_eq!(cfg.effective_horizon_h(), 48);
    }

    #[test]
    fn test_validate_plan_zones_rejects_non_multiple() {
        let yaml = r#"
assets:
  - type: battery
    id: battery
    capacity_kwh: 10.0
    min_soc: 0.10
    round_trip_efficiency: 0.92
planner:
  plan_zones:
    - step_s: 300
      slots: 96
    - step_s: 700
      slots: 96
"#;
        let p: Profile = serde_yaml::from_str(yaml).unwrap();
        let errs = p.validate().unwrap_err();
        assert!(
            errs.iter().any(|e| e.contains("plan_zones")),
            "expected plan_zones violation, got: {errs:?}"
        );
    }

    #[test]
    fn test_validate_plan_zones_accepts_multiples() {
        let yaml = r#"
assets:
  - type: battery
    id: battery
    capacity_kwh: 10.0
    min_soc: 0.10
    round_trip_efficiency: 0.92
planner:
  plan_zones:
    - step_s: 300
      slots: 96
    - step_s: 600
      slots: 96
    - step_s: 900
      slots: 96
"#;
        let p: Profile = serde_yaml::from_str(yaml).unwrap();
        assert!(
            p.validate().is_ok(),
            "300/600/900 are all multiples of 300 — should pass"
        );
    }

    #[test]
    fn test_validate_plan_zones_rejects_zero_step() {
        let yaml = r#"
assets:
  - type: battery
    id: battery
    capacity_kwh: 10.0
    min_soc: 0.10
    round_trip_efficiency: 0.92
planner:
  plan_zones:
    - step_s: 0
      slots: 96
"#;
        let p: Profile = serde_yaml::from_str(yaml).unwrap();
        let errs = p.validate().unwrap_err();
        assert!(
            errs.iter().any(|e| e.contains("plan_zones")),
            "zero step_s should be rejected: {errs:?}"
        );
    }

    #[tokio::test]
    async fn test_yaml_round_trip_plan_zones() {
        // Verify that plan_zones parses correctly from YAML.
        let yaml = r#"
assets:
  - type: battery
    id: battery
    capacity_kwh: 10.0
    min_soc: 0.10
    round_trip_efficiency: 0.92
planner:
  plan_zones:
    - step_s: 3600
      slots: 24
"#;
        let p: Profile = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(p.planner.effective_step_s(), 3600);
        assert_eq!(p.planner.effective_horizon_h(), 24);
    }
}
