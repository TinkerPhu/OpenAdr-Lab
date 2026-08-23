/// `MockSolverPort` — returns a preset `Plan` for every solve request.
/// Lets planning-orchestration code (trigger handling, state wiring) be
/// exercised in tests without invoking HiGHS.
use crate::controller::{SolveRequest, SolverPort};
use crate::entities::plan::Plan;

pub struct MockSolverPort {
    plan: Plan,
}

impl MockSolverPort {
    pub fn returning(plan: Plan) -> Self {
        Self { plan }
    }
}

impl SolverPort for MockSolverPort {
    fn solve(&self, _req: SolveRequest) -> Plan {
        self.plan.clone()
    }
}

/// Always panics — simulates an internal solver bug reaching `spawn_blocking`,
/// for testing `PlanningService::solve_plan`'s `JoinError` fallback path (R-29).
pub struct PanickingSolverPort;

impl SolverPort for PanickingSolverPort {
    fn solve(&self, _req: SolveRequest) -> Plan {
        panic!("simulated solver panic");
    }
}
