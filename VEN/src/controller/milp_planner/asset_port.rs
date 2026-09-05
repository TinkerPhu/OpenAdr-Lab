//! Asset-port types: MILP context/variable/solution structs for the planner boundary.
//!
//! **Struct definitions live here.** Method implementations (declare_vars, constraints,
//! objective, read_solution, from_state) remain in `assets/battery.rs`, `assets/ev.rs`,
//! and `assets/heater.rs` as cross-file inherent impl blocks — valid Rust.
//!
//! ## Architectural invariants (verified by fix-arch-layer-violations, 2026)
//! - `use crate::assets::` in milp_planner/ production code: NONE (invariant holds)
//! - `use crate::assets::` in milp_interactions.rs: NONE
//! - `*Params` structs (`BatteryParams`, `EvParams`, etc.) live in `entities/asset_params`
//! - `assets/*.rs` no longer re-export types from `milp_planner/`; callers import directly

use good_lp::Variable;
use std::collections::HashMap;
// ── Battery MILP types ────────────────────────────────────────────────────────
/// Pre-computed MILP parameters for one battery instance and planning cycle.
/// Built from live state; consumed by `declare_milp_vars` and the constraint/
/// objective methods. Avoids repeated field accesses inside tight solver loops.
#[derive(Debug, Clone)]
pub struct BatteryMilpContext {
    pub e_nom_kwh: f64,
    /// Live SoC × capacity — NOT the profile's initial_soc.
    pub e_init_kwh: f64,
    pub e_min_kwh: f64,
    pub e_max_kwh: f64,
    pub p_ch_max_kw: f64,
    pub p_dis_max_kw: f64,
    /// One-way charge efficiency = √(round_trip_efficiency)
    pub eff_ch: f64,
    /// One-way discharge efficiency = √(round_trip_efficiency)
    pub eff_dis: f64,
    /// Terminal energy reward [EUR/kWh stored at horizon end]. Auto-computed:
    /// mean(c_imp) × round_trip_efficiency. 0.0 disables.
    pub c_terminal_eur_kwh: f64,
}

/// Typed LP variable handles for one battery in the MILP model.
/// `z_active`, `delta_active`, and `delta_ramp` are empty vecs when the
/// corresponding penalty coefficients are zero (feature disabled).
#[derive(Debug, Clone)]
pub struct BatteryMilpVars {
    pub p_ch: Vec<Variable>,
    pub p_dis: Vec<Variable>,
    pub u_bat: Vec<Variable>,
    /// SoC trajectory, len = n + 1 (index 0 = initial SoC, fixed).
    pub e_bat: Vec<Variable>,
    /// Activity indicator per slot (1 = charging or discharging). Empty if startup penalty disabled.
    pub z_active: Vec<Variable>,
    /// Idle→active transition binary per slot boundary. Empty if startup penalty disabled.
    pub delta_active: Vec<Variable>,
    /// |net_bat[t] − net_bat[t−1]| ramp variable. Empty if ramp penalty disabled.
    pub delta_ramp: Vec<Variable>,
    /// Maximum discharge power [kW] — cached from context for cross-asset interactions.
    pub dis_max_kw: f64,
}

/// Per-battery MILP solution readback.
#[derive(Debug, Clone)]
pub struct BatterySolOutput {
    pub p_ch_kw: Vec<f64>,
    pub p_dis_kw: Vec<f64>,
    /// SoC trajectory [kWh], len = n + 1.
    pub e_kwh: Vec<f64>,
}
// ── EV MILP types ─────────────────────────────────────────────────────────────

/// Scheduling mode for the EV in the MILP model.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, PartialEq)]
pub enum EvMilpMode {
    /// Hard energy requirement — must be met within the deadline.
    MustRun,
    /// Soft energy target — controlled by a reward term in the objective.
    MayRun,
    /// EV absent, unplugged, or no charging session — power fixed to zero.
    MustNotRun,
}

/// Pre-computed MILP parameters for one EV charger and planning cycle.
#[derive(Debug, Clone)]
pub struct EvMilpContext {
    pub mode: EvMilpMode,
    /// Per-step availability mask (false forces p_ev[t] = 0).
    pub a_ev: Vec<bool>,
    /// Last step index that counts toward the core energy sum (None = open horizon).
    pub t_dead_step: Option<usize>,
    /// Maximum charge power [kW].
    pub p_max_kw: f64,
    /// Semi-continuous minimum charge power [kW] (prevents trickle charging).
    pub p_min_kw: f64,
    /// Core energy requirement [kWh] from the active session.
    pub e_core_kwh: f64,
    /// Opportunistic headroom = battery_kwh × (1 − soc_target) [kWh].
    pub e_extra_max_kwh: f64,
    /// Reward per kWh of extra opportunistic charging above core [€/kWh].
    pub v_extra_eur_kwh: f64,
    /// One-time reward in EUR for committing to meet the core target (MayRun only; 0.0 otherwise).
    /// Set to e_core_kwh × v_ev_core_eur_kwh so the optimizer commits when tariffs are reasonable.
    pub v_core_eur: f64,
    /// WP4.1 (BL-28) ASAP mode: lateness penalty [€/kWh per hour of delay].
    /// Large enough to dominate tariff spreads → cost-blind front-loading. 0.0 = inactive.
    pub asap_lateness_eur_kwh_h: f64,
    /// WP4.1 (BL-28) OPPORTUNISTIC / *_FREE: when true, `inject_grid_slots`
    /// computes `p_free_cap_kw` and charging is limited to free energy.
    pub free_only: bool,
    /// Per-slot free-energy charge cap [kW]: PV surplus, opened fully when the
    /// import tariff is non-positive. Filled by `inject_grid_slots`; None = no gating.
    pub p_free_cap_kw: Option<Vec<f64>>,
    /// WP4.1-c: reward each charged kWh per slot (v_extra_eur_kwh) instead of
    /// the inert e_ev_extra term. True for all *_FREE / OPPORTUNISTIC / MAX_COST.
    pub reward_per_slot: bool,
    /// WP4.1-c ASAP_FREE: bias the per-slot reward toward earlier slots so free
    /// energy is taken as soon as it appears (never makes later charging unprofitable).
    pub free_early_bias: bool,
    /// WP4.1-c MAX_COST: total charging-cost ceiling [€]; charging cost is
    /// priced at the per-slot import rate (conservative — PV surplus counts
    /// at the same rate). None = no cap.
    pub budget_eur: Option<f64>,
    /// Per-slot import rate [€/kWh] for the budget constraint. Filled by
    /// `inject_grid_slots` when `budget_eur` is set.
    pub c_imp_eur_kwh: Option<Vec<f64>>,
    /// BL-17 comfort bidding: CO2 analogue of `v_extra_eur_kwh`, already monetized via
    /// w_ghg [€/kWh]. 0.0 outside the `ByDeadline`/`Asap` modes.
    pub v_extra_co2_eur_kwh: f64,
    /// BL-17 comfort bidding: CO2 analogue of `v_core_eur`, already monetized via w_ghg
    /// [€] (= e_core_kwh × the curve's fill=0.0 CO2 bid monetized). 0.0 otherwise.
    pub v_core_co2_eur: f64,
}

/// Typed LP variable handles for one EV charger in the MILP model.
#[derive(Debug, Clone)]
pub struct EvMilpVars {
    pub p_ev: Vec<Variable>,
    /// Binary on/off flag per slot (respects availability mask).
    pub z_ev_on: Vec<Variable>,
    /// Binary: 1 when EV core target is met (MayRun only; fixed 0 otherwise).
    pub z_ev_core: Variable,
    /// Total extra energy above core requirement [kWh].
    pub e_ev_extra: Variable,
    /// Startup transition binaries (empty when startup penalty disabled).
    pub delta_ev: Vec<Variable>,
    /// Ramp variables |p_ev[t] − p_ev[t−1]| (empty when ramp penalty disabled).
    pub delta_ev_ramp: Vec<Variable>,
    /// Semi-continuous minimum charge power [kW] — cached for cross-asset interactions.
    pub p_min_kw: f64,
}

/// Per-EV MILP solution readback.
#[derive(Debug, Clone)]
pub struct EvSolOutput {
    pub p_ev_kw: Vec<f64>,
    pub z_ev_on: Vec<f64>,
    pub e_ev_extra_kwh: f64,
    pub z_ev_core: f64,
}

// ── Heater MILP types ─────────────────────────────────────────────────────────

/// Scheduling mode for the heater in the MILP model.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, PartialEq)]
pub enum HeaterMilpMode {
    /// Hard energy target — E[t_dead] ≥ e_target_kwh must hold at the deadline.
    MustRun,
    /// Opportunistic — scheduled by tariffs; soft deadline reward via z_heat_ready.
    MayRun,
    /// Heater absent — all power variables fixed to zero.
    MustNotRun,
}

/// Pre-computed MILP parameters for one heater and planning cycle.
/// Uses a per-step tank energy state trajectory (E[t]) instead of a global energy budget.
#[derive(Debug, Clone)]
pub struct HeaterMilpContext {
    pub mode: HeaterMilpMode,
    /// Deadline step index (None = no hard deadline; autonomous MayRun path).
    pub t_dead_step: Option<usize>,
    /// Mid power level [kW].
    /// Power per switchable stage [kW] = max_kw / n_stages.
    pub p_step_kw: f64,
    /// Number of switchable stages (1 or 2).
    pub n_stages: u8,
    /// Initial tank energy above T_min [kWh]. May be negative when tank is below T_min.
    pub e_init_kwh: f64,
    /// Maximum usable tank energy above T_min [kWh] = (T_max − T_min) × thermal_mass.
    pub e_max_kwh: f64,
    /// Constant per-step thermal demand [kW]: draw_kw + k_loss × (T_mid − ambient).
    pub q_dem_kw: f64,
    /// Target tank energy at deadline [kWh above T_min]. = e_max_kwh in autonomous mode.
    pub e_target_kwh: f64,
    /// Relay switching penalty [EUR/switch event] added to the objective.
    pub lambda_sw_eur: f64,
    /// Last observed hardware stage index (0..=n_stages) at plan time.
    pub initial_y: f64,
    /// Terminal energy reward [EUR/kWh stored at horizon end]. Auto-computed:
    /// mean(c_imp) + c_ctrl_imp_malus. 0.0 disables.
    pub c_terminal_eur_kwh: f64,
    /// Per-slot heater power anchor [kW]. Some(kw) pins the tier binaries for that slot;
    /// None leaves them free. Populated from the previous plan after adoption to prevent
    /// near-future chattering. vec![] or vec![None; n] = no pinning.
    pub anchored_kw: Vec<Option<f64>>,
    /// BL-34: session comfort curve's price at fill=1.0 [EUR/kWh] — a reward on full-tier
    /// operation, competing against the tier penalty. 0.0 when there's no session/curve.
    pub comfort_full_reward_eur_kwh: f64,
    /// BL-17 comfort bidding: session comfort curve's CO2 bid at fill=1.0, monetized via
    /// w_ghg [EUR/kWh] — same reward mechanism as `comfort_full_reward_eur_kwh`, on the CO2
    /// axis instead of price. 0.0 when there's no session/curve.
    pub comfort_full_co2_reward_eur_kwh: f64,
}

/// Typed LP variable handles for one heater in the MILP model.
#[derive(Debug, Clone)]
pub struct HeaterMilpVars {
    /// Integer stage index in [0, n_stages] at slot t; power = p_step_kw × y. len = n.
    pub y_heat: Vec<Variable>,
    /// Binary: 1 when deadline is met (MayRun only; fixed 0 in MustRun / autonomous).
    pub z_heat_ready: Variable,
    /// Continuous: tank energy above T_min [kWh] at slot t. Domain [−e_max, e_max]. len = n.
    pub e_tank: Vec<Variable>,
    /// Continuous ≥ 0: below-minimum soft-violation slack [kWh] at slot t. len = n.
    pub s_low: Vec<Variable>,
    /// Continuous ≥ 0: switching indicator per step. sw[0] measures switch from initial hardware state. len = n.
    pub sw: Vec<Variable>,
    /// Power per stage [kW] — cached from context for cross-asset power balance.
    pub p_step_kw: f64,
    /// Number of switchable stages (1 or 2); max power = p_step_kw × n_stages.
    pub n_stages: u8,
}

/// Per-heater MILP solution readback.
#[derive(Debug, Clone)]
pub struct HeaterSolOutput {
    pub y_heat: Vec<f64>,
    pub z_heat_ready: f64,
    /// Tank energy above T_min [kWh] per slot. len = n.
    pub e_tank_kwh: Vec<f64>,
    #[allow(dead_code)] // solve diagnostic: below-min slack, not consumed by any caller yet
    /// Below-min slack [kWh] per slot. len = n.
    pub s_low_kwh: Vec<f64>,
    #[allow(dead_code)] // solve diagnostic: per-step switching cost, not consumed by any caller yet
    /// Switching cost contribution per step. len = n.
    pub sw: Vec<f64>,
}

/// Below-minimum tank violation penalty [€/kWh]. Used by heater objective (Phase 1).
pub const M_LOW_EUR_PER_KWH: f64 = 10.0;

// R-23: `AssetKind`, `AssetMilpParams`, its variant payload structs, `MilpLoadMode`,
// and the `AssetMilpContext` trait itself now live in the domain-ring
// `controller::asset_milp_port` (domain-level `SolveRequest`/`SolverPort` must not
// reach into this infra module for their own port type). Re-exported here so every
// existing `asset_port::`/`milp_planner::` import path keeps working unchanged.
pub use crate::controller::asset_milp_port::{
    AssetKind, AssetMilpContext, AssetMilpParams, BatteryScalars, EvScalars, HeaterScalars,
    MilpLoadMode, ShiftableLoadScalars,
};

// ── Plan-result helper free functions ─────────────────────────────────────────
// These replace direct calls to Battery/EvCharger/Heater methods in results.rs,
// eliminating the need to import `crate::assets::*` from within milp_planner.

/// Future state map for battery: `{"soc": e_kwh / capacity_kwh}`.
/// Mirrors `Battery::future_state_values()`.
pub fn battery_future_state(e_kwh: f64, capacity_kwh: f64) -> HashMap<String, f64> {
    let soc = (e_kwh / capacity_kwh).clamp(0.0, 1.0);
    HashMap::from([("soc".into(), soc)])
}

/// SoC trajectory from MILP power schedule over `n+1` steps.
/// `dt_h[t]` is the slot duration in hours for slot `t`.
/// Mirrors `EvCharger::soc_trajectory()`.
pub fn ev_soc_trajectory(
    p_ev_kw: &[f64],
    soc_init: f64,
    battery_kwh: f64,
    dt_h: &[f64],
) -> Vec<f64> {
    let n = p_ev_kw.len();
    let mut traj = Vec::with_capacity(n + 1);
    traj.push(soc_init.clamp(0.0, 1.0));
    for t in 0..n {
        let next = traj[t] + p_ev_kw[t] * dt_h[t] / battery_kwh;
        traj.push(next.clamp(0.0, 1.0));
    }
    traj
}

/// Future state map for EV at a given SoC: `{"soc": soc}`.
/// Mirrors `EvCharger::future_state_values_at()`.
pub fn ev_future_state_at(soc: f64) -> HashMap<String, f64> {
    HashMap::from([("soc".into(), soc.clamp(0.0, 1.0))])
}

/// Future state map for heater from tank energy above T_min: `{"temp_c": ...}`.
/// Mirrors `Heater::future_state_values()`.
pub fn heater_future_state(
    e_tank_kwh: f64,
    temp_min_c: f64,
    thermal_mass_kwh_per_c: f64,
) -> HashMap<String, f64> {
    let temp_c = temp_min_c + e_tank_kwh / thermal_mass_kwh_per_c;
    HashMap::from([("temp_c".into(), temp_c)])
}
