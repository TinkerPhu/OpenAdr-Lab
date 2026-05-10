//! Asset-port types: MILP context/variable/solution structs that cross the boundary
//! between the `assets` module and the `controller::milp_planner` module.
//!
//! **Struct definitions live here.** Method implementations (declare_vars, constraints,
//! objective, read_solution, from_state) remain in `assets/battery.rs`, `assets/ev.rs`,
//! and `assets/heater.rs` as cross-file inherent impl blocks — valid Rust.
//!
//! `assets/*.rs` re-export these types via `pub use` for backward compatibility.
//! All code in `controller/milp_planner` imports from `super::asset_port::*` instead
//! of from `crate::assets::*`, which is the architectural invariant enforced by Phase 3.

use chrono::{DateTime, Utc};
use good_lp::{ProblemVariables, Variable};
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

impl EvMilpContext {
    /// Construct from live scalar values — no `AssetState` or `EvCharger` required.
    /// Mirrors the logic in `from_state()` (which stays in `assets/ev.rs`) but
    /// accepts plain scalars extracted directly from the sim snapshot and profile.
    pub fn from_live(
        plugged: bool,
        soc: f64,
        max_charge_kw: f64,
        battery_kwh: f64,
        soc_target: f64,
        min_charge_kw: f64,
        v_ev_extra_eur_kwh: f64,
        n: usize,
        step_s: u64,
        now: DateTime<Utc>,
        ev_session: Option<&crate::entities::device_session::EvSession>,
    ) -> Self {
        if !plugged {
            return Self {
                mode: EvMilpMode::MustNotRun,
                a_ev: vec![false; n],
                t_dead_step: None,
                p_max_kw: max_charge_kw,
                p_min_kw: min_charge_kw,
                e_core_kwh: 0.0,
                e_extra_max_kwh: battery_kwh * (1.0 - soc_target),
                v_extra_eur_kwh: v_ev_extra_eur_kwh,
            };
        }
        if let Some(session) = ev_session {
            let core_kwh = ((session.target_soc - soc) * battery_kwh).max(0.0);
            let mode = if session.soft_deadline {
                EvMilpMode::MayRun
            } else {
                EvMilpMode::MustRun
            };
            let secs = (session.departure_time - now).num_seconds();
            let t_dead =
                (secs / step_s as i64).clamp(0, (n.saturating_sub(1)) as i64) as usize;
            Self {
                mode,
                a_ev: (0..n).map(|t| t <= t_dead).collect(),
                t_dead_step: Some(t_dead),
                p_max_kw: max_charge_kw,
                p_min_kw: min_charge_kw,
                e_core_kwh: core_kwh,
                e_extra_max_kwh: battery_kwh * (1.0 - session.target_soc),
                v_extra_eur_kwh: v_ev_extra_eur_kwh,
            }
        } else {
            // Plugged, no active session: slots available but no obligation
            Self {
                mode: EvMilpMode::MustNotRun,
                a_ev: vec![true; n],
                t_dead_step: None,
                p_max_kw: max_charge_kw,
                p_min_kw: min_charge_kw,
                e_core_kwh: 0.0,
                e_extra_max_kwh: battery_kwh * (1.0 - soc_target),
                v_extra_eur_kwh: v_ev_extra_eur_kwh,
            }
        }
    }
}

// ── Heater MILP types ─────────────────────────────────────────────────────────

/// Scheduling mode for the heater in the MILP model.
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
    pub p_mid_kw: f64,
    /// Full power level [kW] = max_kw.
    pub p_full_kw: f64,
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
    /// Initial heater mid-power binary (1.0 if heater was at mid power last tick).
    pub initial_z_mid: f64,
    /// Initial heater full-power binary (1.0 if heater was at full power last tick).
    pub initial_z_full: f64,
}

/// Typed LP variable handles for one heater in the MILP model.
#[derive(Debug, Clone)]
pub struct HeaterMilpVars {
    /// Binary: 1 = mid power tier active at slot t. len = n.
    pub z_heat_mid: Vec<Variable>,
    /// Binary: 1 = full power tier active at slot t. len = n.
    pub z_heat_full: Vec<Variable>,
    /// Binary: 1 when deadline is met (MayRun only; fixed 0 in MustRun / autonomous).
    pub z_heat_ready: Variable,
    /// Continuous: tank energy above T_min [kWh] at slot t. Domain [−e_max, e_max]. len = n.
    pub e_tank: Vec<Variable>,
    /// Continuous ≥ 0: below-minimum soft-violation slack [kWh] at slot t. len = n.
    pub s_low: Vec<Variable>,
    /// Continuous ≥ 0: switching indicator per step. sw[0] measures switch from initial hardware state. len = n.
    pub sw: Vec<Variable>,
    /// Mid power level [kW] — cached from context for cross-asset power balance.
    pub p_mid_kw: f64,
    /// Full power level [kW] — cached from context for cross-asset power balance.
    pub p_full_kw: f64,
}

/// Per-heater MILP solution readback.
#[derive(Debug, Clone)]
pub struct HeaterSolOutput {
    pub z_heat_mid: Vec<f64>,
    pub z_heat_full: Vec<f64>,
    pub z_heat_ready: f64,
    /// Tank energy above T_min [kWh] per slot. len = n.
    pub e_tank_kwh: Vec<f64>,
    /// Below-min slack [kWh] per slot. len = n.
    pub s_low_kwh: Vec<f64>,
    /// Switching cost contribution per step. len = n.
    pub sw: Vec<f64>,
}

impl HeaterMilpContext {
    /// Construct from live scalar values — no `AssetState` or `Heater` required.
    /// Mirrors the logic in `from_state()` (which stays in `assets/heater.rs`) but
    /// accepts plain scalars extracted directly from the sim snapshot and profile config.
    ///
    /// `q_dem_kw` is the constant per-step thermal demand: `draw_kw + k_loss × (T_mid − ambient)`.
    /// `mid_kw` is the pre-computed mid-tier power (0.0 triggers `max_kw / 2.0` fallback).
    pub fn from_live(
        current_temp_c: f64,
        actual_power_kw: f64,
        temp_min_c: f64,
        temp_max_c: f64,
        mid_kw: f64,
        max_kw: f64,
        thermal_mass_kwh_per_c: f64,
        q_dem_kw: f64,
        lambda_sw: f64,
        n: usize,
        step_s: u64,
        now: DateTime<Utc>,
        heater_target: Option<&crate::entities::device_session::HeaterTarget>,
    ) -> Self {
        let live_mid_kw = if mid_kw > 0.0 { mid_kw } else { max_kw / 2.0 };
        let e_init = (current_temp_c - temp_min_c) * thermal_mass_kwh_per_c;
        let e_max = ((temp_max_c - temp_min_c) * thermal_mass_kwh_per_c).max(0.0);
        let initial_z_mid = if (actual_power_kw - live_mid_kw).abs() < 0.1 {
            1.0
        } else {
            0.0
        };
        let initial_z_full = if (actual_power_kw - max_kw).abs() < 0.1 {
            1.0
        } else {
            0.0
        };
        if let Some(target) = heater_target {
            let e_target =
                ((target.target_temp_c - temp_min_c) * thermal_mass_kwh_per_c).clamp(0.0, e_max);
            let secs = (target.ready_by - now).num_seconds();
            let t_dead =
                (secs / step_s as i64).clamp(0, (n.saturating_sub(1)) as i64) as usize;
            Self {
                mode: HeaterMilpMode::MustRun,
                t_dead_step: Some(t_dead),
                p_mid_kw: live_mid_kw,
                p_full_kw: max_kw,
                e_init_kwh: e_init,
                e_max_kwh: e_max,
                q_dem_kw,
                e_target_kwh: e_target,
                lambda_sw_eur: lambda_sw,
                initial_z_mid,
                initial_z_full,
            }
        } else {
            Self {
                mode: HeaterMilpMode::MayRun,
                t_dead_step: None,
                p_mid_kw: live_mid_kw,
                p_full_kw: max_kw,
                e_init_kwh: e_init,
                e_max_kwh: e_max,
                q_dem_kw,
                e_target_kwh: e_max,
                lambda_sw_eur: lambda_sw,
                initial_z_mid,
                initial_z_full,
            }
        }
    }
}

// ── AssetKind and helper parameter types ─────────────────────────────────────

/// Discriminant for the MILP-capable asset types.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetKind {
    Battery,
    Ev,
    Heater,
}

/// Pre-computed scalar parameters for a battery instance in one planning cycle.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct BatteryScalars {
    pub e_nom_kwh: f64,
    pub e_init_kwh: f64,
    pub e_min_kwh: f64,
    pub e_max_kwh: f64,
    pub p_ch_max_kw: f64,
    pub p_dis_max_kw: f64,
    pub eff_ch: f64,
    pub eff_dis: f64,
}

/// Pre-computed scalar parameters for an EV charger in one planning cycle.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct EvScalars {
    pub p_max_kw: f64,
    pub p_min_kw: f64,
    pub e_core_kwh: f64,
    pub e_extra_max_kwh: f64,
    pub v_extra_eur_kwh: f64,
}

/// Pre-computed scalar parameters for a heater in one planning cycle.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct HeaterScalars {
    pub p_mid_kw: f64,
    pub p_full_kw: f64,
    pub e_init_kwh: f64,
    pub e_max_kwh: f64,
    pub q_dem_kw: f64,
    pub e_target_kwh: f64,
    pub lambda_sw_eur: f64,
}

/// Unified asset MILP parameters — one variant per MILP-capable asset type.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum AssetMilpParams {
    Battery(BatteryScalars),
    Ev(EvScalars),
    Heater(HeaterScalars),
}

// ── AssetMilpContext trait ────────────────────────────────────────────────────

/// Trait implemented by each concrete MILP context type, enabling future
/// trait-object dispatch in the solver phases. Currently used for `AssetKind`
/// discrimination; the `declare_vars`/`constraints`/`objective`/`read_solution`
/// methods forward to the inherent impl methods in the respective assets files.
#[allow(dead_code)]
pub trait AssetMilpContext: Send + Sync {
    fn kind(&self) -> AssetKind;
    fn declare_vars_battery(
        &self,
        _n: usize,
        _c_startup: f64,
        _c_ramp: f64,
        _vars: &mut ProblemVariables,
    ) -> Option<BatteryMilpVars> {
        None
    }
    fn declare_vars_ev(
        &self,
        _n: usize,
        _c_startup: f64,
        _c_ramp: f64,
        _vars: &mut ProblemVariables,
    ) -> Option<EvMilpVars> {
        None
    }
    fn declare_vars_heater(
        &self,
        _n: usize,
        _vars: &mut ProblemVariables,
    ) -> Option<HeaterMilpVars> {
        None
    }
}

impl AssetMilpContext for BatteryMilpContext {
    fn kind(&self) -> AssetKind {
        AssetKind::Battery
    }
    fn declare_vars_battery(
        &self,
        n: usize,
        c_startup: f64,
        c_ramp: f64,
        vars: &mut ProblemVariables,
    ) -> Option<BatteryMilpVars> {
        Some(self.declare_vars(n, c_startup, c_ramp, vars))
    }
}

impl AssetMilpContext for EvMilpContext {
    fn kind(&self) -> AssetKind {
        AssetKind::Ev
    }
    fn declare_vars_ev(
        &self,
        n: usize,
        c_startup: f64,
        c_ramp: f64,
        vars: &mut ProblemVariables,
    ) -> Option<EvMilpVars> {
        Some(self.declare_vars(n, c_startup, c_ramp, vars))
    }
}

impl AssetMilpContext for HeaterMilpContext {
    fn kind(&self) -> AssetKind {
        AssetKind::Heater
    }
    fn declare_vars_heater(&self, n: usize, vars: &mut ProblemVariables) -> Option<HeaterMilpVars> {
        Some(self.declare_vars(n, vars))
    }
}

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
/// Mirrors `EvCharger::soc_trajectory()`.
pub fn ev_soc_trajectory(p_ev_kw: &[f64], soc_init: f64, battery_kwh: f64, dt_h: f64) -> Vec<f64> {
    let n = p_ev_kw.len();
    let mut traj = Vec::with_capacity(n + 1);
    traj.push(soc_init.clamp(0.0, 1.0));
    for t in 0..n {
        let next = traj[t] + p_ev_kw[t] * dt_h / battery_kwh;
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
