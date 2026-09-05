//! `AssetMilpContext` port trait and its contract types (domain ring).
//!
//! R-23: previously defined in `controller::milp_planner::asset_port` (infra ring),
//! but consumed by domain-level `controller::solver_port::SolveRequest`. Moved here
//! so the domain layer no longer reaches into infra for its own port trait;
//! `milp_planner::asset_port` re-exports these for its existing internal callers.
use chrono::{DateTime, Utc};

/// Discriminant for the MILP-capable asset types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AssetKind {
    Battery,
    Ev,
    Heater,
    /// Unlike the other three (one per site, `Option`-shaped pool slot), a
    /// site can have several shiftable loads at once — `MilpVarPool.shiftable`
    /// stays a `Vec`, not a new `Option` slot (`shiftable-load-as-asset`).
    ShiftableLoad,
}

/// MilpLoadMode: scheduling mode shared across EV and heater scalars.
/// Mirrors the per-asset mode enums but decoupled from concrete asset types.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MilpLoadMode {
    MustRun,
    MayRun,
    MustNotRun,
}

/// Pre-computed scalar parameters for a battery instance in one planning cycle.
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
#[derive(Debug, Clone)]
pub struct EvScalars {
    pub mode: MilpLoadMode,
    /// Per-step availability mask (false forces p_ev[t] = 0). len = n.
    pub a_ev: Vec<bool>,
    pub t_dead_step: Option<usize>,
    pub p_max_kw: f64,
    pub p_min_kw: f64,
    pub e_core_kwh: f64,
    pub e_extra_max_kwh: f64,
    pub v_extra_eur_kwh: f64,
    pub v_core_eur: f64,
    /// WP4.1-c MAX_COST: total charging-cost ceiling [€]; None otherwise.
    pub budget_eur: Option<f64>,
}

/// Pre-computed scalar parameters for a heater in one planning cycle.
#[derive(Debug, Clone)]
pub struct HeaterScalars {
    pub mode: MilpLoadMode,
    pub t_dead_step: Option<usize>,
    /// Power per switchable stage [kW] = max_kw / n_stages.
    pub p_step_kw: f64,
    /// Number of switchable stages (1 or 2).
    pub n_stages: u8,
    pub e_init_kwh: f64,
    pub e_max_kwh: f64,
    pub q_dem_kw: f64,
    pub e_target_kwh: f64,
    pub lambda_sw_eur: f64,
    /// Stage index (0..=n_stages) the heater was at on the last real tick.
    pub initial_y: f64,
    /// Terminal energy reward [EUR/kWh]. Mirrors HeaterMilpContext field.
    pub c_terminal_eur_kwh: f64,
}

/// Pre-computed scalar parameters for one shiftable load in one planning
/// cycle — the hard window already resolved to slot indices (`shiftable-load-
/// as-asset`). `valid_start_slots == [0]` encodes an already-`started` load:
/// the start decision is already made, not a MILP choice.
#[derive(Debug, Clone)]
pub struct ShiftableLoadScalars {
    pub power_kw: f64,
    pub duration_slots: usize,
    pub valid_start_slots: Vec<usize>,
}

/// Unified asset MILP parameters — one variant per MILP-capable asset type.
#[derive(Debug, Clone)]
pub enum AssetMilpParams {
    Battery(BatteryScalars),
    Ev(EvScalars),
    Heater(HeaterScalars),
    ShiftableLoad(ShiftableLoadScalars),
    Unknown,
}

/// Port trait for MILP-capable assets. Enables trait-object dispatch in solver phases,
/// eliminating direct imports of concrete asset types from `controller/milp_planner/`.
///
/// **Call order invariant**: `declare_vars_into_pool()` MUST be called before
/// `constraints()` and `objective()`.
pub trait AssetMilpContext: Send + Sync {
    /// Stable identifier matching the SimSnapshot asset map key.
    fn asset_id(&self) -> &str;

    /// Discriminant used for pool-slot dispatch and logging.
    fn asset_kind(&self) -> AssetKind;

    /// Phase A — scalar extraction: return all MILP parameters for this asset,
    /// pre-computed for a planning cycle of `n` slots starting at `now`.
    fn milp_params(&self, n: usize, now: DateTime<Utc>) -> AssetMilpParams;

    /// Phase B — LP variable declaration: add LP variables for this asset to
    /// `vars` and store the resulting typed handles in the appropriate slot of
    /// `pool`. Called once per planning cycle, before constraint/objective building.
    fn declare_vars_into_pool(
        &self,
        n: usize,
        c_startup_eur: f64,
        c_ramp_eur_kw: f64,
        vars: &mut good_lp::ProblemVariables,
        pool: &mut crate::controller::milp_interactions::MilpVarPool,
    );

    /// Phase B — constraints: generate all LP constraints for this asset,
    /// reading its typed vars from `pool`.
    fn constraints(
        &self,
        pool: &crate::controller::milp_interactions::MilpVarPool,
        n: usize,
        dt_h: &[f64],
    ) -> Vec<good_lp::Constraint>;

    /// Phase B — objective contribution: return the cost/comfort expression
    /// for this asset's variables.
    fn objective(
        &self,
        pool: &crate::controller::milp_interactions::MilpVarPool,
        n: usize,
        dt_h: &[f64],
        c_wear_eur_kwh: f64,
        c_startup_eur: f64,
        c_ramp_eur_kw: f64,
    ) -> good_lp::Expression;

    /// Phase A2 — optional per-slot grid-context injection, called after the
    /// global MILP inputs are built (so tariff / PV-forecast / baseline arrays
    /// exist) and before variable declaration. Lets a context derive slot data
    /// it cannot know at construction time — e.g. the OPPORTUNISTIC free-energy
    /// charge cap (WP4.1). Default: no-op.
    fn inject_grid_slots(&mut self, _c_imp_eur_kwh: &[f64], _p_pv_kw: &[f64], _p_base_kw: &[f64]) {}
}
