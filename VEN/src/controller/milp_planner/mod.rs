//! MILP-based HEMS planner — entry point.
//!
//! Builds `MilpInputs` from live state, solves via HiGHS, and translates
//! the solution into a `Plan` with per-slot allocations.
//! See `docs/plans/milp_planner_transition.md` for the design.

// These imports are consumed by `use super::*` in the test submodules (tests/).
// They appear unused in non-test compilation but are the implicit re-export mechanism
// for the wildcard test imports. #[allow] is narrowly scoped to this file.
#[allow(unused_imports)]
use chrono::{DateTime, Duration, Utc};
#[allow(unused_imports)]
use good_lp::solvers::highs::highs;
#[allow(unused_imports)]
use good_lp::{
    constraint, variable, variables, Expression, Solution, SolverModel, Variable,
    WithInitialSolution, WithMipGap, WithTimeLimit,
};
#[allow(unused_imports)]
use tracing::warn;
#[allow(unused_imports)]
use uuid::Uuid;

#[allow(unused_imports)]
use self::asset_port::{
    BatteryMilpContext, BatteryMilpVars, BatterySolOutput, EvMilpContext, EvMilpMode, EvMilpVars,
    EvSolOutput, HeaterMilpContext, HeaterMilpMode, HeaterMilpVars, HeaterSolOutput,
};

pub use self::asset_port::{
    AssetKind, AssetMilpContext, AssetMilpParams, BatteryScalars, EvScalars, HeaterScalars,
    MilpLoadMode,
};
#[allow(unused_imports)]
use crate::controller::milp_interactions::{
    build_interactions, GlobalMilpInputs, GridMilpVars, MilpVarPool, ShiftableLoadMilpVars,
};
use crate::controller::simulator_port::SimSnapshot;
#[allow(unused_imports)]
use crate::entities::asset::PlanTrigger;
use crate::entities::asset_params::AssetParams;
use crate::entities::asset_params::{
    BaseLoadParams, BatteryParams, EvParams, HeaterParams, PvParams,
};
use crate::entities::capacity::OadrCapacityState;
#[allow(unused_imports)]
use crate::entities::device_session::{BaselineOverride, ShiftableLoad};
#[allow(unused_imports)]
use crate::entities::plan::{
    AssetAllocation, CostBreakdown, FlexibilityEnvelope, Plan, PlanSummary, PlanTimeSlot,
    PlanWarning, PlanningHorizon, WarningSeverity,
};
use crate::entities::planner_params::{PlannerObjective, PlannerParams};
use crate::entities::tariff_snapshot::TariffTimeSeries;

pub mod asset_port;
mod envelopes;
mod inputs;
mod results;
mod solver_phase1;
mod solver_phase2;
mod types;

use self::inputs::*;
use self::results::*;
#[cfg(test)]
use self::solver_phase1::*;
use self::solver_phase2::*;
use self::types::*;

fn battery_params(asset_params: &[AssetParams]) -> Option<&BatteryParams> {
    asset_params.iter().find_map(|p| match p {
        AssetParams::Battery(v) => Some(v),
        _ => None,
    })
}

fn ev_params(asset_params: &[AssetParams]) -> Option<&EvParams> {
    asset_params.iter().find_map(|p| match p {
        AssetParams::Ev(v) => Some(v),
        _ => None,
    })
}

fn heater_params(asset_params: &[AssetParams]) -> Option<&HeaterParams> {
    asset_params.iter().find_map(|p| match p {
        AssetParams::Heater(v) => Some(v),
        _ => None,
    })
}

fn pv_params(asset_params: &[AssetParams]) -> Option<&PvParams> {
    asset_params.iter().find_map(|p| match p {
        AssetParams::Pv(v) => Some(v),
        _ => None,
    })
}

fn base_load_params(asset_params: &[AssetParams]) -> Option<&BaseLoadParams> {
    asset_params.iter().find_map(|p| match p {
        AssetParams::BaseLoad(v) => Some(v),
        _ => None,
    })
}

/// Run the MILP planner: build inputs from asset contexts and live state, solve via HiGHS,
/// and translate the solution into a Plan.
/// `objective_override` — when `Some`, overrides the planner default objective.
#[allow(clippy::too_many_arguments)]
pub fn run_planner(
    asset_contexts: Vec<Box<dyn self::asset_port::AssetMilpContext>>,
    assets: &SimSnapshot,
    tariffs: &TariffTimeSeries,
    capacity: &OadrCapacityState,
    alert_windows: &[crate::entities::capacity::AlertWindow],
    planner: &PlannerParams,
    grid_max_import_kw: f64,
    grid_max_export_kw: f64,
    asset_params: &[AssetParams],
    now: DateTime<Utc>,
    trigger: PlanTrigger,
    ev_session: Option<&crate::entities::device_session::EvSession>,
    heater_target: Option<&crate::entities::device_session::HeaterTarget>,
    shiftable_loads: &[ShiftableLoad],
    baseline_override: Option<&BaselineOverride>,
    objective_override: Option<PlannerObjective>,
    pv_forecast_override: Option<f64>,
) -> Plan {
    // Guard: MilpVarPool has one named slot per kind; silently overwrites on duplicates.
    debug_assert!(
        {
            use std::collections::HashSet;
            let kinds: Vec<_> = asset_contexts.iter().map(|c| c.asset_kind()).collect();
            let unique: HashSet<_> = kinds.iter().collect();
            kinds.len() == unique.len()
        },
        "run_planner: duplicate AssetKind in asset_contexts — each kind may appear at most once"
    );

    let objective = objective_override.unwrap_or(planner.objective);
    let battery = battery_params(asset_params);
    let ev = ev_params(asset_params);
    let heater = heater_params(asset_params);
    let pv = pv_params(asset_params);
    let base_load = base_load_params(asset_params);
    let inputs = build_milp_inputs(
        &asset_contexts,
        assets,
        tariffs,
        capacity,
        alert_windows,
        planner,
        grid_max_import_kw,
        grid_max_export_kw,
        pv,
        base_load,
        now,
        shiftable_loads,
        baseline_override,
        pv_forecast_override,
    );
    let p1w = build_phase1_weights(planner, objective);
    let p2w = build_phase2_weights(&inputs, planner);
    match solve_milp_two_phase(
        &inputs,
        &p1w,
        &p2w,
        planner.phase2_epsilon_eur,
        &asset_contexts,
        planner.solver_timeout_s as f64,
    ) {
        Ok((sol, phase1_cost_eur, friction_eur)) => translate_to_plan(
            &sol,
            &inputs,
            &p1w,
            planner,
            now,
            trigger,
            ev_session,
            heater_target,
            shiftable_loads,
            objective,
            phase1_cost_eur,
            friction_eur,
            battery,
            ev,
            heater,
        ),
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
                ev_session,
                heater_target,
                shiftable_loads,
                Some(&inputs),
                format!("MILP solver failed: {e}"),
                objective,
                ev,
                heater,
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
            &req.assets,
            &req.tariffs,
            &req.capacity,
            &req.alert_windows,
            &req.planner,
            req.grid_max_import_kw,
            req.grid_max_export_kw,
            &req.asset_params,
            req.now,
            req.trigger,
            req.ev_session.as_ref(),
            req.heater_target.as_ref(),
            &req.shiftable_loads,
            req.baseline_override.as_ref(),
            req.objective_override,
            req.pv_forecast_override,
        )
    }
}

#[cfg(test)]
mod tests;
