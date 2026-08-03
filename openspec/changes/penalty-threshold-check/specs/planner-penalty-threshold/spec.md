## ADDED Requirements

### Requirement: Peak-demand penalty rule configuration
The system SHALL allow a profile to declare zero or more peak-demand penalty
rules under `planner.penalty_rules`, each with a unique `rule_id`, a positive
`threshold_kw`, a `measurement_window_s` that is a positive multiple of
`plan_step_s`, and a `penalty_eur_per_kw` rate. The feature SHALL be disabled
(no rules active, no behavior change) when `penalty_rules` is empty or
absent, which SHALL be the default.

#### Scenario: Profile with no penalty rules behaves exactly as before
- **WHEN** a profile omits `planner.penalty_rules` (or sets it to an empty list)
- **THEN** the planner adds no penalty slack variables or constraints, and the
  resulting plan is identical to a solve without this feature

#### Scenario: Invalid penalty rule config is rejected at profile load
- **WHEN** a profile declares a penalty rule whose `measurement_window_s` is
  not a positive multiple of `plan_step_s`, or whose `threshold_kw` is not
  positive, or whose `rule_id` duplicates another rule's `rule_id`
- **THEN** `Profile::validate()` returns an error naming the offending rule
  index and field (the same startup-validation mechanism used by every other
  profile invariant, e.g. `plan_zones`)

### Requirement: Planner keeps window peaks at or below threshold when reallocation is cheaper
For each active penalty rule, the planner SHALL partition the planning
horizon into fixed, non-overlapping windows of `measurement_window_s`
starting at the horizon start, and SHALL treat each window's peak grid
import as constrained to `threshold_kw` via a soft penalty: import above the
threshold within a window is permitted but costed at `penalty_eur_per_kw`
per kW of the window's peak that exceeds `threshold_kw`, added to the
solver's objective. The planner SHALL prefer any reallocation of flexible
load that avoids the penalty over paying it, whenever such a reallocation
does not increase total objective cost more than the avoided penalty.

#### Scenario: Load is split across slots to stay under threshold
- **GIVEN** a penalty rule with `threshold_kw = 10.0` and
  `measurement_window_s` equal to one planning slot
- **AND** a flexible demand of 12 kW that could be served across two adjacent
  slots without violating any session deadline
- **WHEN** the planner solves the horizon containing that demand
- **THEN** no slot's net grid import within that window exceeds 10.0 kW
- **AND** the flexible demand is still fully served across the horizon

#### Scenario: Penalty is accepted when reallocation is infeasible or costlier
- **GIVEN** a penalty rule with `threshold_kw = 10.0`
- **AND** a demand that cannot be shifted out of one window (e.g. a MustRun
  device or a hard session deadline coinciding with the window boundary)
  such that the window's peak import must exceed 10.0 kW
- **WHEN** the planner solves that horizon
- **THEN** the resulting plan's `CostBreakdown.c_peak_penalty_eur` is greater
  than zero for that window
- **AND** the plan includes a `PlanWarning` naming the breached rule, the
  window, the peak value, the threshold, and the accepted cost

### Requirement: Penalty outcome is visible in plan output and VEN UI
The system SHALL surface, per solved plan: (a) the accepted penalty cost as
`CostBreakdown.c_peak_penalty_eur`, (b) a `PlanWarning` for every window
still exceeding its threshold after solving, and (c) which active penalty
rules and thresholds apply to the current plan, so a VEN UI client can
render per-slot peak-demand status without deriving it independently.

#### Scenario: Accepted penalty appears in the existing plan-warnings surface
- **GIVEN** a plan whose `CostBreakdown.c_peak_penalty_eur` is greater than
  zero for some window
- **WHEN** a client fetches the plan
- **THEN** `plan.warnings` includes a `Warning`-severity entry describing the
  breached rule, window, peak value, threshold, and accepted cost

#### Scenario: VEN UI shows which slots were reallocated to avoid a threshold
- **GIVEN** a plan produced under a profile with at least one active penalty
  rule
- **WHEN** the VEN UI Planner tab renders the plan
- **THEN** the Decision Matrix displays a per-slot peak-demand indicator
  reflecting each slot's import relative to the active threshold(s), naming
  the rule on hover
- **AND** when no penalty rule is active for the plan, the Decision Matrix
  renders with no peak-demand row, unchanged from before this capability
