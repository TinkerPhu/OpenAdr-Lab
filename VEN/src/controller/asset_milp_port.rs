//! `AssetMilpContext` port trait and its contract types (domain ring).
//!
//! R-23: previously defined in `controller::milp_planner::asset_port` (infra ring),
//! but consumed by domain-level `controller::solver_port::SolveRequest`. Moved here
//! so the domain layer no longer reaches into infra for its own port trait;
//! `milp_planner::asset_port` re-exports these for its existing internal callers.
use std::collections::HashMap;

use chrono::{DateTime, Utc};

/// Which part of its objective an asset contributes. Phase 1 minimises cost; phase 2 caps that
/// same cost and minimises friction on top; the marginal-cost pass prices the cost alone.
#[derive(Debug, Clone, Copy)]
pub enum ObjectivePart<'a> {
    /// The asset's economic terms (wear, rewards, comfort, penalties for missing a promise).
    Cost(&'a CostWeights),
    /// The asset's operational friction (startups, ramps, relay switching, tier preference),
    /// priced by the costs the context was built with.
    Friction,
}

/// The objective-dependent price an asset's cost part reads. Today one: what a kWh through a
/// storage asset costs in wear.
#[derive(Debug, Clone, Copy)]
pub struct CostWeights {
    /// Wear per kWh charged or discharged by a storage asset [EUR/kWh].
    pub c_storage_wear_eur_kwh: f64,
}

/// An asset's own friction prices, set when its context is built from the planner parameters.
/// A kind uses the fields that apply to it and leaves the rest at 0.0.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FrictionCosts {
    /// Per run start [EUR].
    pub startup_eur: f64,
    /// Per kW of power change between consecutive slots [EUR/kW].
    pub ramp_eur_kw: f64,
    /// Per slot at full stage over a lower one [EUR/slot].
    pub tier_penalty_eur: f64,
}

/// How a solve declares an asset's variables.
#[derive(Debug, Clone, Copy)]
pub enum DeclareMode<'a> {
    /// The plan: every decision free. `friction` adds the auxiliaries the friction objective
    /// prices (phase 2); without it they are not declared (phase 1).
    Plan { friction: bool },
    /// The marginal-cost pass: every mode decision fixed to the asset's own winning solution,
    /// as a continuous variable, because HiGHS returns no duals for a model with any integer
    /// column (R-98).
    Pinned(&'a AssetSolution),
}

impl DeclareMode<'_> {
    /// The friction an asset prices in this declaration: its own costs when phase 2 plans,
    /// none otherwise (phase 1 declares no friction auxiliaries, and the marginal-cost pass
    /// prices fixed decisions).
    pub fn friction(&self, own: &FrictionCosts) -> FrictionCosts {
        match self {
            DeclareMode::Plan { friction: true } => *own,
            DeclareMode::Plan { friction: false } | DeclareMode::Pinned(_) => {
                FrictionCosts::default()
            }
        }
    }
}

/// What one asset's part of a solved model is: its net power, what it plans to be in, how it
/// stores energy if it does, what it warns about, and the kind's own detail.
#[derive(Debug, Clone)]
pub struct AssetSolution {
    pub asset_id: String,
    /// Net power per slot [kW], positive = import: what the asset adds to the site balance.
    pub net_power_kw: Vec<f64>,
    /// The asset's planned state per slot (the keys its live `state_values` use). Shorter than
    /// the plan, or empty, when the asset forecasts no state for those slots.
    pub planned_state: Vec<HashMap<String, f64>>,
    /// The two legs and the stored energy of an asset that stores energy for the site.
    pub storage: Option<StorageLegs>,
    /// What the asset reports about its own solution (e.g. a promise it cannot keep).
    pub warnings: Vec<crate::entities::plan::PlanWarning>,
    /// The kind's own readback, matched only by the kind itself (pinning, warm start).
    pub detail: AssetSolutionDetail,
}

/// A storage asset's charge and discharge legs and its stored energy.
#[derive(Debug, Clone)]
pub struct StorageLegs {
    pub charge_kw: Vec<f64>,
    pub discharge_kw: Vec<f64>,
    /// Stored energy at every step boundary [kWh], len = n + 1.
    pub energy_kwh: Vec<f64>,
}

/// One kind's own solution readback. A closed set next to `AssetState`: each variant is read by
/// its own kind only (`scripts/audit_ven_architecture.py` keeps it that way).
#[derive(Debug, Clone)]
pub enum AssetSolutionDetail {
    Battery {
        /// Charge (1) / discharge (0) direction per slot, as solved.
        direction: Vec<f64>,
    },
    Ev(crate::controller::milp_planner::asset_port::EvSolOutput),
    Heater(crate::controller::milp_planner::asset_port::HeaterSolOutput),
    ShiftableLoad {
        /// The start choice per valid start slot, as solved.
        starts: Vec<f64>,
    },
    /// A forecast participant (PV, base load): its net power is all there is to read back.
    Forecast,
}

/// What a forecast participant contributes to the site: generation (PV) or load (base load).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastRole {
    Generation,
    Load,
}

/// Whether the plan schedules an asset or only follows its forecast.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Participation<'a> {
    /// The plan decides the asset's power and dispatches it as allocations.
    Dispatched,
    /// The asset follows its own forecast (`forecast_kw`, a magnitude per slot); the plan
    /// balances and prices around it but never dispatches it: no allocation, no planned power,
    /// no place among the planned assets or in the controllable-import malus.
    Forecast {
        role: ForecastRole,
        forecast_kw: &'a [f64],
    },
}

/// How an asset's allocation in a slot is priced in the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllocationBasis {
    /// A load: its grid share at the import tariff, its PV-surplus share at the export tariff
    /// (the revenue forgone); valued at the import tariff.
    Load,
    /// Storage: priced as a load while charging, at export revenue while discharging; valued at
    /// the export tariff.
    Storage,
}

/// The window, energy and power range an asset declares for the session it is serving, from
/// which the plan reports how flexible that session is.
#[derive(Debug, Clone, PartialEq)]
pub struct FlexibilityWindow {
    pub energy_needed_kwh: f64,
    pub power_min_kw: f64,
    pub power_max_kw: f64,
    pub window_start: DateTime<Utc>,
    pub window_end: DateTime<Utc>,
}

/// A discharge leg one asset offers to a cross-asset interaction.
pub struct DischargeLeg<'a> {
    pub p_dis: &'a [good_lp::Variable],
    pub max_kw: f64,
}

/// An on/off indicator one asset offers to a cross-asset interaction.
pub struct OnIndicator<'a> {
    pub z_on: &'a [good_lp::Variable],
    /// The smallest power the asset runs at while on [kW].
    pub p_min_kw: f64,
}

/// One asset's variables in one solve, and everything the solve asks of them. Built by
/// `AssetMilpContext::declare`, so the asset that owns the variables is the only code that knows
/// their shape.
pub trait DeclaredAsset {
    fn asset_id(&self) -> &str;

    /// The asset's net power in slot `t` [kW], positive = import: its term in the site balance.
    fn net_power_expr(&self, t: usize) -> good_lp::Expression;

    /// Every constraint on the asset's own variables that `slot_constraints` does not add.
    fn constraints(&self, n: usize, dt_h: &[f64]) -> Vec<good_lp::Constraint>;

    /// The rows that belong to slot `t` alone, added right after that slot's power-balance row
    /// (PV's `used <= forecast`, where the grid's own PV bound used to sit, so HiGHS receives the
    /// rows in the order it always did).
    fn slot_constraints(&self, _t: usize) -> Vec<good_lp::Constraint> {
        Vec::new()
    }

    /// The asset's contribution to the objective part a solve asks for.
    fn objective(&self, part: ObjectivePart<'_>, n: usize, dt_h: &[f64]) -> good_lp::Expression;

    /// Initial values for phase 2 from the asset's own phase-1 solution: a feasible start, so
    /// HiGHS does not have to find one.
    fn warm_start(&self, own: &AssetSolution, n: usize) -> Vec<(good_lp::Variable, f64)>;

    /// Read the asset's part of a solved model.
    fn read(&self, solution: &dyn good_lp::Solution, n: usize) -> AssetSolution;

    /// The discharge leg, for an asset that can discharge into the site.
    fn discharge_leg(&self) -> Option<DischargeLeg<'_>> {
        None
    }

    /// The on/off indicator, for an asset that switches on and off at a minimum power.
    fn on_indicator(&self) -> Option<OnIndicator<'_>> {
        None
    }

    /// Whether the plan dispatches this asset (see `Participation`). Forecast participants
    /// count in the site balance but not in the controllable-import malus.
    fn dispatched(&self) -> bool {
        true
    }
}

/// A pinned 0/1 decision: the solved value rounded to the nearest of the two.
pub fn pinned_binary(solved: Option<f64>) -> f64 {
    if solved.unwrap_or(0.0) > 0.5 {
        1.0
    } else {
        0.0
    }
}

/// Port trait for MILP-capable assets: what the planner asks of each asset it plans, without
/// knowing which kind it is. Implemented in `assets/` by each kind's context.
pub trait AssetMilpContext: Send + Sync {
    /// The roster id this context was built for.
    fn asset_id(&self) -> &str;

    /// Declare this asset's variables for one solve. Every variable and bound is declared by
    /// this one function in every mode, so the planned and the priced model cannot drift apart
    /// (R-98).
    fn declare<'a>(
        &'a self,
        n: usize,
        mode: DeclareMode<'_>,
        vars: &mut good_lp::ProblemVariables,
    ) -> Box<dyn DeclaredAsset + 'a>;

    /// Where this asset stands in claiming a slot's PV surplus in the plan: lower claims first,
    /// equal ranks keep planner input order.
    fn surplus_claim_rank(&self) -> u8;

    /// How this asset's allocations are priced in the plan.
    fn allocation_basis(&self) -> AllocationBasis;

    /// Whether the plan schedules this asset or only follows its forecast.
    fn participation(&self) -> Participation<'_> {
        Participation::Dispatched
    }

    /// The session this asset is serving as of `now`, for the plan's session flexibility.
    /// `None` when it serves none.
    fn flexibility_window(&self, _now: DateTime<Utc>) -> Option<FlexibilityWindow> {
        None
    }

    /// What the asset can tell before the solve, given the plan's import tariff per slot
    /// (e.g. a budget too small for its target).
    fn input_warnings(&self, _c_imp_eur_kwh: &[f64]) -> Vec<crate::entities::plan::PlanWarning> {
        Vec::new()
    }

    /// Phase A2 — optional per-slot grid-context injection, called after the
    /// global MILP inputs are built (so the import rates and the forecast surplus
    /// exist) and before variable declaration. Lets a context derive slot data
    /// it cannot know at construction time — e.g. the OPPORTUNISTIC free-energy
    /// charge cap (WP4.1). `forecast_surplus_kw` is signed (`MilpInputs`). Default: no-op.
    fn inject_grid_slots(&mut self, _c_imp_eur_kwh: &[f64], _forecast_surplus_kw: &[f64]) {}

    /// Lets a test read the concrete context it built, e.g. what an EV derived from a session.
    #[cfg(test)]
    fn as_any(&self) -> &dyn std::any::Any;
}
