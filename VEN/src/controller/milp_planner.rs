//! MILP-based HEMS planner — entry point.
//!
//! This module will contain the full Mixed-Integer Linear Program formulation
//! for the HEMS energy scheduler.  See `docs/plans/milp_planner_transition.md`
//! for the design.
//!
//! Currently a stub: returns an empty plan so the rest of the controller
//! (dispatcher, monitor, timeline) compiles and runs while the MILP
//! implementation is being built.

use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

use crate::entities::asset::PlanTrigger;
use crate::entities::capacity::OadrCapacityState;
use crate::entities::energy_packet::EnergyPacket;
use crate::entities::plan::{
    Plan, PlanStep, PlanSummary, PlanningHorizon, PlanWarning,
};
use crate::entities::tariff_snapshot::TariffTimeSeries;
use crate::profile::Profile;
use crate::simulator::SimState;

/// Run the MILP planner and return a new Plan + audit trail.
///
/// Stub: returns an empty plan covering the configured horizon.
/// The MILP formulation will replace this body in the next implementation phase.
pub fn run_planner(
    _assets: &SimState,
    _tariffs: &TariffTimeSeries,
    packets: &[EnergyPacket],
    _capacity: &OadrCapacityState,
    profile: &Profile,
    now: DateTime<Utc>,
    trigger: PlanTrigger,
) -> (Plan, Vec<PlanStep>) {
    let step_s = profile.planner.plan_step_s;
    let horizon_h = profile.planner.plan_horizon_h;

    let horizon_end = now + Duration::seconds((horizon_h as f64 * 3600.0) as i64);
    let total_steps = ((horizon_h as f64 * 3600.0) / step_s as f64) as usize;

    let horizon = PlanningHorizon {
        start_time: now,
        end_time: horizon_end,
        step_size_s: step_s,
        num_steps: total_steps,
        far_horizon: horizon_end,
    };

    let warning = PlanWarning {
        severity: crate::entities::plan::WarningSeverity::Info,
        packet_id: None,
        message: "MILP planner not yet implemented — returning empty plan.".into(),
        suggested_action: None,
    };

    let plan = Plan {
        id: Uuid::new_v4(),
        created_at: now,
        trigger,
        horizon,
        slots: vec![],
        summary: PlanSummary::default(),
        envelopes: vec![],
        packets: packets.to_vec(),
        warnings: vec![warning],
        steps: vec![],
    };

    (plan, vec![])
}
