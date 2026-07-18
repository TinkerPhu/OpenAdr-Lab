## ADDED Requirements

### Requirement: Plan carries its solve outcome
Every `Plan` produced by the planner SHALL carry a `solve_status` field with value
`OPTIMAL` or `INFEASIBLE`, set at the point the `Plan` is constructed, reflecting
whether it came from a successful MILP solve or from the infeasibility fallback path.

#### Scenario: Successful solve is reported as optimal
- **WHEN** the MILP solver returns `Ok((solution, phase1_cost_eur, friction_eur))` for
  a feasible planning horizon
- **THEN** the resulting `Plan.solve_status` equals `OPTIMAL`

#### Scenario: Infeasible solve is reported distinctly from a successful one
- **WHEN** the MILP solver returns an `Err` for an unsolvable set of constraints and
  the planner falls back to `fallback_plan`
- **THEN** the resulting `Plan.solve_status` equals `INFEASIBLE`
- **AND** the `Plan.warnings` list still contains a Critical warning carrying the
  infeasibility reason, unchanged from today's behaviour

### Requirement: Solve outcome and objective values are streamed over SSE
The `PlanReady` variant of `PlannerEvent` (`GET /plan/events`) SHALL carry
`solve_status`, `objective_eur`, and `friction_eur` for the plan that triggered the
event, using the same values as the corresponding `Plan`.

#### Scenario: SSE payload matches the plan it announces
- **WHEN** a new `Plan` is adopted and a `PlanReady` event is emitted
- **THEN** the event's `solve_status`, `objective_eur`, and `friction_eur` fields
  equal the adopted `Plan`'s corresponding fields exactly

### Requirement: UI distinguishes an infeasible plan from a merely-warned plan
The VEN UI SHALL render a status indicator driven by `plan.solve_status` that is
visually and structurally distinct from the generic plan-warnings badge, so an
infeasible/fallback plan cannot be mistaken for a plan with only minor warnings.

#### Scenario: Infeasible plan shows a distinct status chip
- **WHEN** the Planner tab receives a `Plan` with `solve_status: "INFEASIBLE"`
- **THEN** `PlanHeaderBar` renders an infeasible-status chip separate from the
  warnings-count badge
- **AND** the existing Critical warning describing the infeasibility reason is still
  shown when the warnings list is expanded

#### Scenario: Optimal plan shows no infeasible chip
- **WHEN** the Planner tab receives a `Plan` with `solve_status: "OPTIMAL"`
- **THEN** `PlanHeaderBar` does not render the infeasible-status chip
