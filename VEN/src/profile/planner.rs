//! `PlannerConfig` — the `planner:` block of a VEN profile YAML.
//!
//! Split out of `profile/schema.rs`, which sat at 490/500 production lines with
//! this struct alone accounting for ~170 of them. It is the fastest-growing
//! config block in the profile (objective weights, friction penalties, adoption
//! gate, solver tuning), so leaving it in `schema.rs` meant every new planner
//! knob had to fight the file-size cap — which is what forced the 2026-08-27
//! revert of the configurable MIP gap. Re-exported from `schema` so existing
//! `profile::schema::PlannerConfig` paths keep resolving.

use crate::entities::plan::PlanZone;
use crate::entities::planner_params::PenaltyRuleParams;
use crate::entities::PlannerObjective;
use serde::Deserialize;

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

    /// WP3.2 — SIMPLE level 1 ("mild") import cap as a fraction of the
    /// contractual limit (0.0–1.0, default 0.5). Levels 2 and 3 have fixed
    /// semantics (baseline cap / zero cap) — see `entities::capacity::SimpleWindow`.
    #[serde(default = "super::defaults::default_simple_level1_import_cap_pct")]
    pub simple_level1_import_cap_pct: f64,

    /// WP4.1 (BL-28) — ASAP mode lateness penalty [€/kWh per hour of delay].
    /// Large by design so ASAP is effectively cost-blind (default 10.0).
    #[serde(default = "super::defaults::default_asap_lateness_eur_kwh_h")]
    pub asap_lateness_eur_kwh_h: f64,

    /// WP4.1 (BL-28) — reward per kWh of free-energy charging in
    /// OPPORTUNISTIC / *_FREE modes (default 0.10; must exceed the feed-in
    /// tariff so consuming PV surplus beats exporting it).
    #[serde(default = "super::defaults::default_v_ev_free_charge")]
    pub v_ev_free_charge_eur_kwh: f64,

    /// WP4.4 (BL-07) — policy for slots beyond tariff coverage. One of
    /// LAST_KNOWN, HEURISTIC_FORECAST (default; stub → LAST_KNOWN until
    /// Phase 5 BL-14), DEFER_TO_FLEXIBLE, SAFE_AVERAGE.
    #[serde(default = "super::defaults::default_stale_rate_policy")]
    pub stale_rate_policy: crate::entities::design_vocabulary::StaleRatePolicy,

    /// WP4.4 — SAFE_AVERAGE percentile over the known import rates
    /// (0.0–1.0, nearest-rank; default 0.8).
    #[serde(default = "super::defaults::default_stale_rate_safe_pctl")]
    pub stale_rate_safe_pctl: f64,

    /// WP6.3 (BL-09) — active peak-demand penalty rules. Empty (default) =
    /// feature disabled, planner behavior unchanged. Validated in
    /// `Profile::validate` — see that method for the rules.
    #[serde(default)]
    pub penalty_rules: Vec<PenaltyRuleParams>,
}
