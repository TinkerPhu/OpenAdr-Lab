//! MILP-based HEMS planner — entry point.
//!
//! Builds `MilpInputs` from live state, solves via HiGHS, and translates
//! the solution into a `Plan` with per-slot allocations.
//! See `docs/plans/milp_planner_transition.md` for the design.

// The test submodules under tests/ reach their fixtures through `use super::*`,
// so this module is their import surface. Gated on `cfg(test)` rather than
// carried as production imports behind `#[allow(unused_imports)]`: in a test
// build the children use them, in a production build they do not exist, and
// neither case needs an allow.
use chrono::{DateTime, Utc};
use tracing::warn;

use crate::entities::asset::PlanTrigger;
use crate::entities::device_session::BaselineOverride;
use crate::entities::plan::Plan;

/// The test submodules under tests/ reach their fixtures through
/// `use super::*`, so this module is their import surface. These three are
/// the only names they need that production code here does not already
/// import; gated on `cfg(test)` rather than carried behind
/// `#[allow(unused_imports)]`, so an entry that stops being used becomes a
/// warning instead of staying invisible.
#[cfg(test)]
mod test_prelude {
    pub use chrono::Duration;
    // Still named by the test module through `use super::*`, but no longer
    // by production code here: `run_planner` takes a `GridSignals` now.
    pub use crate::entities::capacity::OadrCapacityState;
    pub use crate::entities::tariff_snapshot::TariffTimeSeries;
    pub use good_lp::{constraint, Solution, SolverModel};
}
#[cfg(test)]
use test_prelude::*;

pub use self::asset_port::{
    AllocationBasis, AssetMilpContext, AssetSolution, AssetSolutionDetail, CostWeights,
    DeclareMode, DeclaredAsset, DischargeLeg, FlexibilityWindow, ForecastRole, FrictionCosts,
    ObjectivePart, OnIndicator, Participation, StorageLegs,
};
pub(crate) use self::types::{ForecastMilpContext, ShiftableLoadMilpContext};
use crate::entities::planner_params::{PlannerObjective, PlannerParams};

pub mod asset_port;
mod input_stages;
mod inputs;
mod session_flexibility;
// Defines `probe!`; must precede the solver modules that use it.
#[macro_use]
mod model_probe;
mod model_skeleton;
pub(crate) mod penalty;
mod results;
mod solver_duals;
mod solver_phase1;
mod solver_phase2;
mod stale_rates;
mod types;

use self::inputs::*;
use self::results::*;

pub(crate) use self::results::fallback_plan;
#[cfg(test)]
use self::solver_duals::*;
#[cfg(test)]
use self::solver_phase1::*;
use self::solver_phase2::*;
use self::types::*;

/// Run the MILP planner: build inputs from asset contexts and live state, solve via HiGHS,
/// and translate the solution into a Plan.
/// `objective_override` — when `Some`, overrides the planner default objective.
#[allow(clippy::too_many_arguments)]
pub fn run_planner(
    mut asset_contexts: Vec<Box<dyn self::asset_port::AssetMilpContext>>,
    grid: &crate::entities::grid_signals::GridSignals,
    planner: &PlannerParams,
    grid_max_import_kw: f64,
    grid_max_export_kw: f64,
    now: DateTime<Utc>,
    trigger: PlanTrigger,
    baseline_override: Option<&BaselineOverride>,
    objective_override: Option<PlannerObjective>,
    diurnal_import_ref: Option<&lab_core::time_series::TimeSeries>,
    diurnal_co2_ref: Option<&lab_core::time_series::TimeSeries>,
) -> Plan {
    // Every asset is planned by its own id: its variables, its pinned decisions and its plan
    // result are all looked up by it, so two contexts with one id would silently share them.
    debug_assert!(
        {
            let mut ids: Vec<&str> = asset_contexts.iter().map(|c| c.asset_id()).collect();
            ids.sort_unstable();
            ids.windows(2).all(|w| w[0] != w[1])
        },
        "run_planner: two asset contexts report the same asset id"
    );

    // The forecast participants (PV, base load) come first, the assets the plan dispatches after
    // them; each group keeps its input order. The model declares PV's used generation where the
    // grid's own PV variable used to be, before the penalty slacks (`ModelSkeleton::declare`).
    asset_contexts.sort_by_key(|c| matches!(c.participation(), Participation::Dispatched));
    let objective = objective_override.unwrap_or(planner.objective);
    let inputs = build_milp_inputs(
        grid,
        planner,
        &inputs::SiteInputs {
            grid_max_import_kw,
            grid_max_export_kw,
            baseline_override,
        },
        &inputs::StaleRateRefs {
            import: diurnal_import_ref,
            co2: diurnal_co2_ref,
        },
        now,
        &asset_contexts,
    );
    // WP4.1 (BL-28): give contexts the per-slot grid data they cannot know at
    // construction time (e.g. the OPPORTUNISTIC free-energy charge cap).
    let forecast_surplus_kw = inputs.forecast_surplus_kw();
    for ctx in asset_contexts.iter_mut() {
        ctx.inject_grid_slots(&inputs.c_imp_eur_kwh, &forecast_surplus_kw);
    }
    let p1w = build_phase1_weights(planner, objective);
    match solve_milp_two_phase(
        &inputs,
        &p1w,
        planner.phase2_epsilon_eur,
        &asset_contexts,
        planner.solver_timeout_s as f64,
        planner.phase2_solver_timeout_s as f64,
    ) {
        Ok((sol, phase1_cost_eur, friction_eur, marginal_cost_eur_per_kwh, phase_report)) => {
            translate_to_plan(
                &sol,
                &inputs,
                &asset_contexts,
                &p1w,
                planner,
                now,
                trigger,
                objective,
                phase1_cost_eur,
                friction_eur,
                &marginal_cost_eur_per_kwh,
                Some(phase_report),
            )
        }
        Err(e) => {
            // BL-25 (WP2.3): construct the reserved PlanInfeasible variant at
            // its real boundary. SolverPort::solve stays infallible by design
            // (always returns a usable Plan — see solver_port.rs) so this is
            // logged, not propagated as an error; the resulting fallback Plan
            // already carries a Critical PlanWarning with the same detail.
            let domain_err = crate::entities::DomainError::PlanInfeasible(e.to_string());
            warn!(error = %domain_err, "MILP solver failed");
            fallback_plan(
                planner,
                now,
                trigger,
                &asset_contexts,
                Some(&inputs),
                format!("MILP solver failed: {e}"),
                objective,
            )
        }
    }
}

/// Concrete `SolverPort` implementation backed by the two-phase MILP solver.
/// Thin wrapper: unpacks `SolveRequest` and forwards to `run_planner`, which
/// remains the single source of truth for solve behaviour and keeps its own
/// test suite (`tests/`) untouched by this port.
pub struct MilpSolver;

impl crate::controller::SolverPort for MilpSolver {
    fn solve(&self, req: crate::controller::SolveRequest) -> Plan {
        run_planner(
            req.asset_contexts,
            &req.grid,
            &req.planner,
            req.grid_max_import_kw,
            req.grid_max_export_kw,
            req.now,
            req.trigger,
            req.baseline_override.as_ref(),
            req.objective_override,
            req.diurnal_import_eur_kwh.as_ref(),
            req.diurnal_co2_g_kwh.as_ref(),
        )
    }
}

#[cfg(test)]
mod tests;
