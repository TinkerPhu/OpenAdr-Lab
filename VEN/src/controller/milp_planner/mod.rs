//! MILP-based HEMS planner — entry point.
//!
//! Builds `MilpInputs` from live state, solves via HiGHS, and translates
//! the solution into a `Plan` with per-slot allocations.
//! See `docs/plans/milp_planner_transition.md` for the design.

use chrono::{DateTime, Duration, Utc};
use good_lp::solvers::highs::highs;
use good_lp::{
    constraint, variable, variables, Expression, Solution, SolverModel, Variable,
    WithInitialSolution, WithMipGap, WithTimeLimit,
};
use tracing::warn;
use uuid::Uuid;

use crate::assets::battery::{Battery, BatteryMilpContext};
use crate::assets::ev::{EvCharger, EvMilpContext, EvMilpMode, EvState};
use crate::assets::heater::{Heater, HeaterMilpContext, HeaterMilpMode, HeaterState};
use crate::assets::{AssetState, PvInverter};
use crate::controller::milp_interactions::{
    build_interactions, GlobalMilpInputs, GridMilpVars, MilpVarPool, ShiftableLoadMilpVars,
};
use crate::controller::simulator_port::SimSnapshot;
use crate::entities::asset::PlanTrigger;
use crate::entities::capacity::OadrCapacityState;
use crate::entities::device_session::{BaselineOverride, ShiftableLoad};
use crate::entities::plan::{
    AssetAllocation, CostBreakdown, FlexibilityEnvelope, Plan, PlanSummary, PlanTimeSlot,
    PlanWarning, PlanningHorizon, WarningSeverity,
};
use crate::entities::tariff_snapshot::TariffTimeSeries;
use crate::profile::{PlannerObjective, Profile};


mod types;
mod inputs;
mod solver_phase1;
mod solver_phase2;
mod envelopes;
mod results;

use self::inputs::*;
use self::results::*;
use self::solver_phase2::*;
use self::types::*;
#[cfg(test)]
use self::solver_phase1::*;

/// Run the MILP planner: build inputs from live state, solve via HiGHS,
/// and translate the solution into a Plan.
/// `objective_override` — when `Some`, overrides the profile's default objective.
pub fn run_planner(
    assets: &SimSnapshot,
    tariffs: &TariffTimeSeries,
    capacity: &OadrCapacityState,
    profile: &Profile,
    now: DateTime<Utc>,
    trigger: PlanTrigger,
    ev_session: Option<&crate::entities::device_session::EvSession>,
    heater_target: Option<&crate::entities::device_session::HeaterTarget>,
    shiftable_loads: &[ShiftableLoad],
    baseline_override: Option<&BaselineOverride>,
    objective_override: Option<PlannerObjective>,
) -> Plan {
    let objective = objective_override.unwrap_or(profile.planner.objective);
    let inputs = build_milp_inputs(
        assets,
        tariffs,
        capacity,
        profile,
        now,
        ev_session,
        heater_target,
        shiftable_loads,
        baseline_override,
    );
    let p1w = build_phase1_weights(profile, objective);
    let p2w = build_phase2_weights(&inputs, profile);
    match solve_milp_two_phase(&inputs, &p1w, &p2w, profile.planner.phase2_epsilon_eur) {
        Ok((sol, phase1_cost_eur, friction_eur)) => translate_to_plan(
            &sol,
            &inputs,
            &p1w,
            profile,
            now,
            trigger,
            ev_session,
            heater_target,
            shiftable_loads,
            objective,
            phase1_cost_eur,
            friction_eur,
        ),
        Err(e) => {
            warn!("MILP solver failed: {e}");
            fallback_plan(
                profile,
                now,
                trigger,
                ev_session,
                heater_target,
                shiftable_loads,
                Some(&inputs),
                format!("MILP solver failed: {e}"),
                objective,
            )
        }
    }
}

#[cfg(test)]
mod tests;
