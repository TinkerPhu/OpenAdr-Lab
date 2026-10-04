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
    /// Live SoC at plan time, reported by the EV itself (seeds the plan's EV SoC forecast).
    pub soc_init: f64,
    /// The highest state of charge this vehicle will accept (0..1) — its configured
    /// charge limit, which `EvCharger::capability_inner` enforces absolutely by
    /// reporting zero import capability at or above it.
    ///
    /// Declared here so the model cannot plan energy the charger will refuse. It used
    /// to be implicit, and the two answers disagreed: `capability_inner` stopped at
    /// the limit while `ev_comfort::ev_energy_segments` priced bands all the way to a
    /// full pack, so a plan would promise charge the asset then rejected — ven-2 was
    /// planned to 0.998 against a 0.85 limit while a week of measurements never once
    /// exceeded 0.850.
    pub soc_max: f64,
    /// Per-step availability mask (false forces p_ev[t] = 0).
    pub a_ev: Vec<bool>,
    /// `ev-usage-forecast`: exogenous SoC changes the plan must project but
    /// cannot decide (the drop when the car returns). `None` under
    /// `usage_sim`/no usage schedule — the pre-forecast behaviour.
    pub soc_drops: Option<ExogenousSocDrops>,
    /// Every charging obligation this EV must meet inside the horizon, one per
    /// stated or predicted departure. Empty = no obligation (open horizon):
    /// charge what the bids justify, whenever.
    ///
    /// R-92: this is deliberately a list, not a scalar deadline. A household EV
    /// has a *sequence* of departures, and a 30-48 h horizon routinely holds two.
    pub obligations: Vec<EvObligation>,
    /// Usable pack size [kWh]. Converts charging power into state of charge in
    /// the SoC balance, so the model reasons in the same unit the asset and the
    /// UI use.
    pub battery_kwh: f64,
    /// Maximum charge power [kW].
    pub p_max_kw: f64,
    /// Semi-continuous minimum charge power [kW] (prevents trickle charging).
    pub p_min_kw: f64,
    /// `ev-comfort-piecewise-core`: the user's comfort curve as priced energy
    /// bands from the current SoC to full. Empty for the free/opportunistic
    /// modes, which never read the curve and price per slot instead.
    pub segments: Vec<EvEnergySegment>,
    /// Opportunistic headroom = battery_kwh × (1 − soc_target) [kWh].
    pub e_extra_max_kwh: f64,
    /// Reward per kWh of extra opportunistic charging above core [€/kWh].
    pub v_extra_eur_kwh: f64,
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
}

/// Typed LP variable handles for one EV charger in the MILP model.
#[derive(Debug, Clone)]
pub struct EvMilpVars {
    pub p_ev: Vec<Variable>,
    /// State of charge (0..1) at every step boundary, len = n + 1. R-93: the
    /// plan's SoC curve is solved, not reconstructed afterwards. Index 0 is
    /// pinned to the live reading.
    pub soc_ev: Vec<Variable>,
    /// Per-slot part of a predicted trip's SoC drop that the floor absorbs
    /// (len = n, upper-bounded by that slot's drop, zero-width where no trip
    /// ends). Without it a drop deeper than the pack would be infeasible instead
    /// of floored, which is what the live tick's `apply_return_drop` does.
    pub drop_unmet: Vec<Variable>,
    /// Per-obligation shortfall in SoC terms (len = obligations). Lets an
    /// unreachable target be expressed by the model and reported, instead of
    /// pre-clamped by the caller and invisible.
    pub shortfall_soc: Vec<Variable>,
    /// Binary on/off flag per slot (respects availability mask).
    pub z_ev_on: Vec<Variable>,
    /// `ev-comfort-piecewise-core`: one continuous variable per priced energy
    /// band, bounded by that band's kWh. Empty for the free/opportunistic modes.
    pub e_seg: Vec<Variable>,
    /// Total energy for the modes that price per slot rather than per band
    /// (free/opportunistic/MAX_COST), bounded by `e_extra_max_kwh`.
    pub e_ev_extra: Variable,
    /// Startup transition binaries (empty when startup penalty disabled).
    pub delta_ev: Vec<Variable>,
    /// Ramp variables |p_ev[t] − p_ev[t−1]| (empty when ramp penalty disabled).
    pub delta_ev_ramp: Vec<Variable>,
    /// Semi-continuous minimum charge power [kW] — cached for cross-asset interactions.
    pub p_min_kw: f64,
    /// Usable pack size [kWh] — cached so the readback can turn a solved SoC
    /// shortfall back into kWh without the context (same reason `p_min_kw` is here).
    pub battery_kwh: f64,
}

/// Per-EV MILP solution readback.
#[derive(Debug, Clone)]
pub struct EvSolOutput {
    pub p_ev_kw: Vec<f64>,
    /// Solved state-of-charge curve (0..1), len = n + 1.
    pub soc_ev: Vec<f64>,
    /// Solved per-obligation shortfall [kWh], in obligation order.
    pub shortfall_kwh: Vec<f64>,
    pub z_ev_on: Vec<f64>,
    pub e_ev_extra_kwh: f64,
    /// Energy bought from the priced bands [kWh] — what the comfort curve
    /// actually justified, which is how much of a firm requirement was met.
    pub e_seg_kwh: f64,
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
pub fn battery_future_state(e_kwh: f64, capacity_kwh: f64) -> HashMap<String, f64> {
    let soc = crate::entities::asset_params::battery_soc_from_energy(e_kwh, capacity_kwh);
    HashMap::from([("soc".into(), soc)])
}

/// One band of EV energy priced at a single marginal bid
/// (`ev-comfort-piecewise-core`).
///
/// The user's comfort curve is a marginal-value curve over state of charge:
/// "the most I will pay for the next kWh, at this much charge". Walking it
/// produces these bands — together they span the energy from the EV's current
/// SoC to full, each with its own €/kWh. The planner buys the bands whose bid
/// beats their cost and stops, which is what lets a charge degrade gracefully
/// instead of being an all-or-nothing block.
///
/// Bids are non-increasing across the bands (enforced at the API boundary by
/// `services::comfort::validate_curve`), which is what keeps the valuation
/// concave and therefore solvable with continuous variables only — no binary.
#[derive(Debug, Clone, PartialEq)]
pub struct EvEnergySegment {
    /// Energy in this band [kWh].
    pub kwh: f64,
    /// What the user bids for each kWh in it [€/kWh], CO2 bid already
    /// monetized in.
    pub eur_per_kwh: f64,
}

/// One charging obligation: "this EV must hold this much energy by this step".
///
/// R-92/R-93: the planner used to carry a single `(t_dead_step, e_required_kwh)`
/// pair per EV — twice over, once on `EvMilpContext` and once on `MilpInputs` —
/// so only the next departure could ever be targeted. One list of these replaces
/// both copies, and the solver emits one constraint per entry.
#[derive(Debug, Clone, PartialEq)]
pub struct EvObligation {
    /// Last step index that counts toward this obligation.
    pub deadline_step: usize,
    /// State of charge (0..1) the vehicle MUST hold at `deadline_step`, whatever
    /// the user bid — a guarantee, not a reward. An obligation exists only for a
    /// firm target: a soft deadline states no obligation and lets its comfort
    /// bids decide how far to charge.
    pub target_soc: f64,
    /// The `EvSession` this obligation came from, when it came from one. `None`
    /// for an obligation the EV's own predicted schedule produced
    /// (`ev-usage-forecast`), which has no session. Carried so a shortfall can
    /// name which request it belongs to.
    pub session_id: Option<uuid::Uuid>,
}

/// Exogenous, decision-independent state-of-charge changes at specific slots
/// (`ev-usage-forecast`: the drop when the car returns from a predicted trip).
///
/// This is the one thing the EV's energy accounting cannot derive from the
/// plan's own charging decisions — it happens *to* the asset while it is away,
/// so it is supplied as data rather than inferred.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExogenousSocDrops {
    /// SoC fraction to subtract at each slot; zero for almost every slot.
    pub drop_frac_per_slot: Vec<f64>,
    /// A drop never takes the projected SoC below this fraction.
    pub floor_frac: f64,
}

/// Future state map for EV at a given SoC: `{"soc": soc}`.
pub fn ev_future_state_at(soc: f64) -> HashMap<String, f64> {
    HashMap::from([(
        "soc".into(),
        crate::entities::asset_params::ev_soc_clamped(soc),
    )])
}

/// Future state map for heater from tank energy above T_min: `{"temp_c": ...}`.
pub fn heater_future_state(
    e_tank_kwh: f64,
    temp_min_c: f64,
    thermal_mass_kwh_per_c: f64,
) -> HashMap<String, f64> {
    let temp_c = crate::entities::asset_params::heater_temp_c_from_energy(
        e_tank_kwh,
        temp_min_c,
        thermal_mass_kwh_per_c,
    );
    HashMap::from([("temp_c".into(), temp_c)])
}

#[cfg(test)]
mod tests {
    use super::*;

    // These three functions are what `planned_state.rs` actually calls to turn a
    // solved energy variable into the state the plan displays, and they carried no
    // tests of their own — the coverage sat on the asset-side duplicates R-73
    // deleted. The conversions themselves are pinned in
    // `entities::asset_params`; what is asserted here is the key each map is read
    // by, which is the part a caller depends on.

    #[test]
    fn battery_future_state_is_read_by_soc() {
        let m = battery_future_state(5.0, 10.0);
        assert_eq!(m.len(), 1);
        assert!((m["soc"] - 0.5).abs() < 1e-9);
    }

    #[test]
    fn ev_future_state_at_is_read_by_soc() {
        let m = ev_future_state_at(0.65);
        assert_eq!(m.len(), 1);
        assert!((m["soc"] - 0.65).abs() < 1e-9);
    }

    #[test]
    fn heater_future_state_is_read_by_temp_c() {
        // 2 kWh above an 18 C floor at 2 kWh/C -> 19 C.
        let m = heater_future_state(2.0, 18.0, 2.0);
        assert_eq!(m.len(), 1);
        assert!((m["temp_c"] - 19.0).abs() < 1e-9);
    }
}
