# Spec Delta

## Purpose

Lets a simulated EV's known future leave/return schedule reach the planner as genuine per-slot
availability data, so a charging plan can correctly anticipate a predicted departure — and the
return after it — instead of only reacting once the departure has already happened or relying on
a one-sided deadline that can never express a round trip.

## ADDED Requirements

### Requirement: `usage_forecast` is an alternative to `usage_sim`, never both
An EV asset SHALL support a `usage_forecast` configuration as an alternative to `usage_sim`. A
profile SHALL NOT declare both on the same EV. Absent either, the EV's behavior is unaffected by
this capability, exactly as when neither is declared today.

#### Scenario: Declaring both is rejected
- **WHEN** a profile declares both `usage_sim` and `usage_forecast` on the same EV asset
- **THEN** the profile fails validation with an error naming both sections

### Requirement: `usage_forecast` simulates physically exactly like `usage_sim`
An EV configured with `usage_forecast` SHALL undergo the identical daily leave/return simulation as
`usage_sim` — the same weekday/weekend schedule fields, the same forced-unplug window, and the same
state-of-charge drop applied once at return, floored at the configured minimum.

#### Scenario: Physical behavior is unchanged from usage_sim
- **WHEN** an EV is configured with `usage_forecast` using the same weekday/weekend schedule
  values as an equivalent `usage_sim` configuration
- **THEN** its live plugged/unplugged transitions and state-of-charge drop occur at the same
  instants and by the same amounts as `usage_sim` would produce

### Requirement: The planner receives the EV's true predicted future availability
For each plan slot within the planning horizon, an EV configured with `usage_forecast` SHALL
report to the planner whether it is predicted to be available for charging or export at that
slot's instant, computed from the same schedule that governs its physical simulation. A slot
within a predicted trip's leave-to-return window SHALL be reported as unavailable (zero feasible
charge and discharge power); a slot outside any predicted trip SHALL be reported with the asset's
normal capability.

#### Scenario: A future predicted trip is reflected in the plan's availability
- **WHEN** an EV configured with `usage_forecast` has a predicted trip whose leave and return both
  fall within the current planning horizon
- **THEN** every plan slot within `[leave_at, return_at)` reports zero feasible charge and
  discharge power, and slots outside that window report the asset's normal capability

#### Scenario: Multiple predicted trips within one horizon are each reflected
- **WHEN** the planning horizon spans more than one predicted trip for the same EV (for example, a
  trip tonight and another the following evening)
- **THEN** each trip's own leave-to-return window is independently reported as unavailable in the
  plan, with no limit on how many such windows a single horizon can contain

### Requirement: The predicted state-of-charge drop is reflected in the plan's energy accounting
The plan's projected state-of-charge for an EV configured with `usage_forecast` SHALL include the
predicted trip's state-of-charge drop, applied once, at the first plan slot at or after the
predicted return instant — independent of whatever charging the plan schedules for that slot.

#### Scenario: The plan's projected SoC reflects the predicted drop at return
- **WHEN** a plan slot boundary is at or after a predicted trip's return instant
- **THEN** the plan's projected state-of-charge at that boundary reflects the configured drop,
  floored at the configured minimum, in addition to any charging scheduled that slot

### Requirement: `engage_charge_planning` targets the next predicted departure directly
When `engage_charge_planning` is enabled under `usage_forecast`, the planner SHALL be given a
charging target (the EV's configured target state of charge) and deadline (the next predicted
leave instant within the planning horizon) without creating or modifying any charge session. When
disabled, no such target or deadline SHALL be introduced; the EV is charged opportunistically
within its truthfully reported available windows.

#### Scenario: Enabled — planner targets the next predicted departure
- **WHEN** `engage_charge_planning` is enabled and the next predicted leave instant falls within
  the planning horizon
- **THEN** the plan targets the EV's configured state of charge by that leave instant, and no
  charge session is created or altered as a result

#### Scenario: Disabled — no target is introduced
- **WHEN** `engage_charge_planning` is disabled
- **THEN** the plan schedules the EV's charging opportunistically, with no deadline or target
  introduced by this capability, exactly as when `usage_forecast` is enabled with
  `engage_charge_planning` off

### Requirement: A real charge session's target always takes priority; predicted availability never yields to one
When a real user- or VTN-created charge session is active for the same EV, its target state of
charge and deadline SHALL take priority over anything `usage_forecast`'s `engage_charge_planning`
would otherwise have contributed. The EV's predicted availability SHALL always be asserted to the
planner regardless of whether a real session is active, and SHALL NOT be overridden by one.

#### Scenario: A real session's target overrides the forecast's target
- **WHEN** a real user- or VTN-created charge session is active for an EV that also has
  `usage_forecast` with `engage_charge_planning` enabled
- **THEN** the plan targets the real session's state of charge and deadline, not the value
  `usage_forecast` would have contributed

#### Scenario: A real session cannot make a predicted-unavailable slot available
- **WHEN** a real charge session's deadline falls within or after a slot the EV is predicted to be
  unavailable for
- **THEN** that slot still reports zero feasible charge and discharge power in the plan

### Requirement: An unreachable predicted deadline is surfaced, not silently dropped or rejected
When `engage_charge_planning`'s target cannot be reached because the EV's predicted available
slots before the deadline do not offer enough chargeable time, the system SHALL deliver the best
feasible charge and SHALL surface a warning, using the same notification mechanism already used
for an unreachable cost-limited charging target. The system SHALL NOT reject the configuration or
refuse to produce a plan.

#### Scenario: An unreachable predicted deadline still produces a best-effort plan and a warning
- **WHEN** the EV's predicted available charging time before the next predicted departure is
  insufficient to reach the configured target state of charge
- **THEN** the plan delivers the best feasible state of charge by that deadline and a warning is
  surfaced, rather than the plan being rejected or the shortfall going unreported
