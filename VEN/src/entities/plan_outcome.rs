//! How a plan came out: the solve's status and the warnings it carries, split from `plan.rs`
//! (the 500-line cap) and re-exported there, so every reader keeps `entities::plan::*`. Owns
//! the vocabulary only; the planner decides which status and warnings a plan gets.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Outcome of the MILP solve that produced a `Plan`.
///
/// `Infeasible` covers every solve failure the two-phase solver can hit
/// (genuinely infeasible, unbounded, or any other solver error) — the
/// codebase has no distinct heuristic-solve path today (verified: all
/// "heuristic" references are `AssetHeuristics`/BL-14 learned load profiles,
/// unrelated to solver fallback), so `fallback_plan` is synonymous with
/// infeasibility, not a substitute heuristic solve.
///
/// `TimeLimit`/`GapLimit` (GB-31) cover the two ways a *feasible* solve can
/// still be non-optimal: HiGHS stopped at `solver_timeout_s` before
/// certifying optimality, or it stopped once the solution was within
/// `planner.mip_gap_target` of the best known bound (the more common case in
/// practice) — both read straight from `good_lp`'s `SolutionStatus`, not
/// hardcoded. See that profile field's doc comment for what's still not
/// exposed (the achieved gap as a number, as opposed to this coarser
/// within-target/not classification).
///
/// `Simulated` is not a solve at all: the plan forecasts what the opportunistic objective's
/// dispatch would do, stepped through the simulator (openspec `opportunistic-objective` D6). It
/// has slots to show and is never compared on cost with a solved plan.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SolveStatus {
    Optimal,
    TimeLimit,
    GapLimit,
    Infeasible,
    Simulated,
}

/// Severity of a plan warning.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WarningSeverity {
    Info,
    Warning,
    Critical,
}

/// Stable, machine-readable classification of a `PlanWarning` (GB-25).
///
/// Covers exactly the 6 real construction sites in
/// `controller::milp_planner::results` today, plus `Other` as a catch-all for
/// any future warning that hasn't earned its own variant yet. `services::notify`
/// dedups new-vs-carried-over warnings on `kind` (not `message`, which can
/// carry per-cycle interpolated numbers).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WarningKind {
    /// `results::fallback_plan` — the MILP solver failed and this is the infeasibility fallback.
    SolverInfeasible,
    /// WP4.4 (BL-07) — the tariff/CO2 rate used for one or more slots was filled by the
    /// StaleRatePolicy rather than a fresh VTN-reported value.
    StaleRateEstimate,
    /// WP4.1-c (BL-28) — the MAX_COST budget could not be met at the target SoC.
    BudgetShortfall,
    /// Grid import/export capacity was exceeded in one or more slots; the solver used slack.
    CapacityViolation,
    /// WP6.3 (BL-09) — a penalty-rule threshold was still exceeded after solving (penalty accepted).
    PeakPenaltyExceeded,
    /// A **firm**-deadline EV session whose guaranteed energy the plan cannot deliver in the
    /// window it has (car away for too much of it, charger too slow, deadline too close). The
    /// wire name predates `ev-comfort-piecewise-core`, which narrowed the meaning to this one
    /// case: a soft request that charges less than its target is the comfort curve working as
    /// asked, not an unmet obligation (GB-41).
    EvCoreEnergyUnmet,
    /// A request's target is above what the asset may reach (an EV's charge limit): the plan
    /// holds it to the limit and says so, rather than reporting the gap as an unmet guarantee
    /// (R-140).
    TargetAboveLimit,
    /// A heater target the plan cannot reach by its ready-by time: a thermostat latch
    /// holds the heater off for part of the window, or power and time are too short.
    HeaterTargetUnmet,
    /// Reserved for future warning sites not yet classified above.
    Other,
}

/// A warning generated during planning (§6.5).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct PlanWarning {
    pub severity: WarningSeverity,
    #[serde(default = "WarningKind::default_other")]
    pub kind: WarningKind,
    pub message: String,
    pub suggested_action: Option<String>,
}

impl WarningKind {
    pub(crate) fn default_other() -> Self {
        WarningKind::Other
    }
}

impl SolveStatus {
    pub(crate) fn default_optimal() -> Self {
        SolveStatus::Optimal
    }

    /// Whether the solve produced a plan to dispatch. A failed one carries objective 0
    /// and no slots, which must never read as "cheapest" to anyone comparing plans.
    pub fn solved(self) -> bool {
        self != SolveStatus::Infeasible
    }
}
