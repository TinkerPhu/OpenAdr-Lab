use crate::entities::asset_params::{
    AssetParams, BaseLoadParams, BatteryParams, EvParams, HeaterParams, PvParams,
};
use crate::entities::plan::PlanZone;
use crate::entities::PlannerObjective;
use serde::Deserialize;

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
    pub grid: GridConfig,
    #[serde(default)]
    pub history: HistoryConfig,
}

/// WP1.2/WP1.3 (Phase 1, A-1) — persistent history sampling + retention.
#[derive(Debug, Clone, Deserialize)]
pub struct HistoryConfig {
    #[serde(default = "super::defaults::default_history_enabled")]
    pub enabled: bool,
    #[serde(default = "super::defaults::default_history_retention_days")]
    #[allow(dead_code)] // read by WP1.3's retention-pruning task, not yet landed
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

#[derive(Debug, Clone, Deserialize)]
pub struct PvConfig {
    #[serde(default = "super::defaults::default_asset_id_pv")]
    pub id: String,
    #[serde(default = "super::defaults::default_pv_rated")]
    pub rated_kw: f64,
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

/// Physical grid connection limits — meter / main breaker hard ceiling.
/// The MILP uses these as `p_imp_max_phys_kw` / `p_exp_max_phys_kw`.
/// When no OpenADR capacity event is active these also act as the contractual limit.
#[derive(Debug, Clone, Deserialize)]
pub struct GridConfig {
    /// Physical import limit at the meter or main breaker (kW).
    /// Default: 25.0 kW — typical residential 3-phase 32 A supply.
    #[serde(default = "super::defaults::default_max_import_kw")]
    pub max_import_kw: f64,
    /// Physical export limit (inverter / grid-tie maximum) (kW).
    /// Default: 10.0 kW.
    #[serde(default = "super::defaults::default_max_export_kw")]
    pub max_export_kw: f64,
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
    #[serde(default = "super::defaults::default_plan_step")]
    pub plan_step_s: u64,
    /// Total planning horizon in hours (default 48). Ignored when `plan_zones` is set.
    #[serde(default = "super::defaults::default_plan_horizon_h")]
    pub plan_horizon_h: u64,
    /// Seconds between periodic replanning cycles (default 300).
    #[serde(default = "super::defaults::default_replan_interval")]
    pub replan_interval_s: u64,

    /// Scales the energy cost term (import tariff cost − export revenue).
    /// 1.0 = full economic optimization. 0.0 = ignore energy cost (e.g. pure GHG mode).
    #[serde(default = "super::defaults::default_w_energy")]
    pub w_energy: f64,
    /// Weight on GHG emissions: equivalent €/kgCO₂ added to objective.
    /// 0.0001 ≈ €100/tonne CO₂ — a light carbon price signal.
    #[serde(default = "super::defaults::default_w_ghg")]
    pub w_ghg: f64,
    /// Penalty per kWh of total grid exchange (import + export), in €/kWh.
    /// Drives the optimizer toward self-consumption. Default: 0.0 (disabled).
    #[serde(default)]
    pub w_grid: f64,
    /// Battery cycling wear cost in €/kWh charged or discharged.
    /// Prevents excessive cycling when arbitrage margin is thin.
    #[serde(default = "super::defaults::default_bat_wear")]
    pub c_bat_wear_eur_kwh: f64,
    /// Startup penalty per EV charging run [€/run].
    /// Breaks degeneracy: encourages one contiguous charging block rather than fragmented slots.
    #[serde(default = "super::defaults::default_ev_startup")]
    pub c_ev_startup_eur: f64,
    /// Startup penalty per battery charge/discharge mode transition [€/transition].
    /// Encourages contiguous charge and discharge blocks rather than scattered spikes.
    #[serde(default = "super::defaults::default_bat_startup")]
    pub c_bat_startup_eur: f64,
    /// Ramp penalty per kW of EV power change between consecutive slots [€/kW].
    /// Penalises |p_ev[t] - p_ev[t-1]|; keeps charging at a stable power level.
    #[serde(default = "super::defaults::default_ev_ramp")]
    pub c_ev_ramp_eur_kw: f64,
    /// Ramp penalty per kW of battery net-power change between consecutive slots [€/kW].
    /// Penalises |(p_ch[t]−p_dis[t]) − (p_ch[t−1]−p_dis[t−1])|; smooths battery power.
    #[serde(default = "super::defaults::default_bat_ramp")]
    pub c_bat_ramp_eur_kw: f64,
    /// Penalty per kWh of battery discharge co-occurring with EV charging in slots where
    /// PV surplus (p_pv − p_base) ≥ p_ev_min_kw [€/kWh]. Discourages unnecessary battery
    /// cycling when free PV power is available to cover the EV load.
    /// Set to 0.0 to disable. Default: 0.5.
    #[serde(default = "super::defaults::default_bat_ev_coexist")]
    pub c_bat_ev_coexist_eur_kwh: f64,
    /// Scales contractual limit violation penalties. 1.0 = normal; 0.0 = disabled.
    #[serde(default = "super::defaults::default_w_viol")]
    pub w_viol: f64,
    /// Per-kWh penalty for exceeding the contractual import limit (€/kWh slack).
    /// Default: 10 000 — high enough that no realistic energy saving outweighs slack cost.
    #[serde(default = "super::defaults::default_pen_imp")]
    pub pen_imp_eur_kwh: f64,
    /// Per-kWh penalty for exceeding the contractual export limit (€/kWh slack).
    /// Default: 10 000 — symmetric with import penalty.
    #[serde(default = "super::defaults::default_pen_exp")]
    pub pen_exp_eur_kwh: f64,
    /// Reward per kWh of EV charging above the core energy requirement (€/kWh).
    /// Incentivises opportunistic top-up charging when tariffs are low.
    #[serde(default = "super::defaults::default_v_ev_extra")]
    pub v_ev_extra_eur_kwh: f64,
    /// One-time reward (EUR) per kWh of core energy target for committing to a
    /// soft-deadline EV session (MayRun mode). Must exceed the expected charging
    /// cost for the optimizer to choose z_ev_core = 1. Default: 1.0 EUR/kWh
    /// (~3–5× typical peak tariff), overridable per-VEN in profile YAML.
    #[serde(default = "super::defaults::default_v_ev_core")]
    pub v_ev_core_eur_kwh: f64,
    /// Soft penalty per slot for using the heater's full power tier over mid tier [€/slot].
    /// Breaks ties in favour of mid tier (e.g. 3 kW) over full tier (e.g. 6 kW) when tariff
    /// savings are equal. Must be small relative to actual energy cost differences.
    /// Default: 0.001 EUR/slot.
    #[serde(default = "super::defaults::default_w_tier_penalty")]
    pub w_tier_penalty_eur: f64,
    /// Phase 1 penalty [€/kWh] on controllable-asset import exceeding free PV surplus.
    /// Covers all controllable assets as a group (heater + EV + net battery + shiftables).
    /// When the total controllable load exceeds `max(0, p_pv − p_base)` the excess kWh
    /// is penalised at this rate, discouraging pre-storage arbitrage beyond what PV provides.
    /// Set to ~0.20–0.25 to prefer mid-tier when PV exactly covers it.
    /// Default: 0.0 (disabled — existing behaviour preserved).
    #[serde(default = "super::defaults::default_c_ctrl_imp_malus")]
    pub c_ctrl_imp_malus_eur_kwh: f64,
    /// Optimization objective preset. Selects weight ratios for the MILP solver.
    /// Set to `custom` to use the individual weight fields above directly.
    #[serde(default)]
    pub objective: PlannerObjective,

    /// Minimum objective improvement (EUR) required to replace the current plan on a
    /// Periodic replan trigger. Hard triggers (RateChange, CapacityChange, Alert,
    /// UserRequest, AssetStateChange) always force adoption.
    /// 0.0 = always adopt. Default: 0.20 (suppress churn when improvement is marginal).
    #[serde(default = "super::defaults::default_plan_adoption_threshold")]
    pub plan_adoption_threshold_eur: f64,

    /// Time constant (seconds) for linear decay of `plan_adoption_threshold_eur`.
    /// As time flows the rolling planning window shifts, so a new plan cannot always
    /// beat the old one in absolute EUR even when it is genuinely optimal for current
    /// conditions. The effective threshold at the adoption gate is:
    ///   effective = threshold × max(0, 1 − elapsed_s / decay_s)
    /// After `decay_s` seconds the effective threshold reaches 0.0 and any new plan
    /// is accepted. 0.0 = no decay (full threshold always applied).
    /// Default: 1500 s (5× replan_interval_s).
    #[serde(default = "super::defaults::default_plan_adoption_decay")]
    pub plan_adoption_decay_s: f64,
    /// Cost cap slack for Phase 2 lexicographic solve [EUR]. Phase 2 minimises
    /// operational friction (startup/ramp/switching/tier) subject to:
    ///   phase1_cost ≤ c_star + phase2_epsilon_eur
    /// Set to 0.0 to disable Phase 2 (single-phase solve). Default: 0.02.
    #[serde(default = "super::defaults::default_phase2_epsilon")]
    pub phase2_epsilon_eur: f64,

    /// HiGHS solver time limit per phase in seconds. Default: 60.
    #[serde(default = "super::defaults::default_solver_timeout_s")]
    pub solver_timeout_s: u64,

    /// Seconds the planning loop sleeps after startup before the first plan.
    /// Allows event polling to populate tariff rates first. Default: 5.
    #[serde(default = "super::defaults::default_planning_initial_delay_s")]
    pub planning_initial_delay_s: u64,

    /// Per-extra-switch surcharge [EUR] added to the effective acceptance threshold.
    /// Periodic replans that introduce more heater relay operations than the current plan
    /// must overcome this additional cost penalty before being adopted.
    /// 0.0 = disabled (default). Suggested: match `switching_penalty_eur`.
    #[serde(default)]
    pub gate_switch_penalty_eur: f64,
}
