# Spec Delta

## Purpose

The planner reasons about an EV's state of charge across the planning horizon as
part of the plan it solves, rather than reconstructing it afterwards, so that a
charging target binds at its own deadline and a recharge after a predicted return
can be planned.

## ADDED Requirements

### Requirement: The planned state of charge is part of the solved plan

The plan SHALL carry a state-of-charge value for the EV at every step of the
horizon, derived within the optimisation rather than reconstructed from the power
schedule afterwards. The state of charge at a step SHALL equal the previous step's
value, plus the energy charged in that step, minus any energy consumed by a trip
ending in that step, and SHALL never fall below the configured floor nor exceed a
full pack.

#### Scenario: Charging raises the projected state of charge

- **WHEN** a plan schedules charging in a slot
- **THEN** the projected state of charge at the following step is higher by the energy charged, expressed as a fraction of the pack

#### Scenario: A predicted return lowers it

- **WHEN** the horizon contains a predicted trip return with an expected consumption
- **THEN** the projected state of charge falls at that step by the expected amount

#### Scenario: The floor is respected

- **WHEN** a predicted trip would consume more than the vehicle holds
- **THEN** the projected state of charge settles at the configured floor rather than below it

#### Scenario: The projection starts from the live reading

- **WHEN** a plan is produced
- **THEN** its first state-of-charge value is the state of charge the EV asset itself reported at plan time

### Requirement: A charging target binds at its own deadline

A charging target SHALL be expressed as a requirement on the EV's state of charge
at the step its deadline falls in. Energy charged after that step SHALL NOT
contribute to meeting it.

#### Scenario: The target is met at the deadline

- **WHEN** a firm target is stated for a deadline inside the horizon and the window can deliver it
- **THEN** the projected state of charge at that deadline step is at least the target

#### Scenario: Charging after the deadline does not count

- **WHEN** the plan charges only after the deadline step
- **THEN** the target is reported as unmet

#### Scenario: A target beyond the horizon does not constrain this plan

- **WHEN** a target's deadline falls after the end of the horizon
- **THEN** it imposes no requirement on this plan

### Requirement: Several targets may be stated for one EV

The planner SHALL accept more than one charging target for a single EV, each with
its own deadline step and its own required state of charge, and SHALL satisfy each
at its own deadline so far as the vehicle's availability allows.

#### Scenario: Two targets at two deadlines

- **WHEN** two targets are stated for the same EV at different deadline steps inside the horizon
- **THEN** the projected state of charge meets each target at its own deadline step

#### Scenario: Charge is carried across an intervening absence

- **WHEN** a later target cannot be met from the energy available after the vehicle returns, but can be met by charging before it leaves
- **THEN** the plan charges before the departure to the extent that the charge survives the trip, and the later target is met

#### Scenario: A single target behaves as before

- **WHEN** exactly one target is stated
- **THEN** the plan is the same as it would be with the single-deadline model

### Requirement: An unreachable target is reported, never infeasible

Where a target cannot be reached by its deadline, the plan SHALL still be
produced, charging SHALL be maximised within the available window, and the
shortfall SHALL be reported as the amount of energy by which the target was
missed. A charging target SHALL NOT make the site plan infeasible.

#### Scenario: An unreachable target still yields a plan

- **WHEN** a firm target demands more energy than the available slots before its deadline can deliver
- **THEN** a plan is produced, the window is charged as fully as possible, and the shortfall is reported

#### Scenario: The reported shortfall is the amount actually missed

- **WHEN** a target is missed by a known amount of energy
- **THEN** the reported shortfall equals that amount

#### Scenario: A met target reports no shortfall

- **WHEN** every stated target is met at its deadline
- **THEN** no shortfall is reported

### Requirement: A recharge after a predicted return can be planned

Where the horizon contains a predicted return, the planner SHALL be able to
schedule charging after that return to serve a later target, rather than only
avoiding charging while the vehicle is away.

#### Scenario: Charging is planned after a return

- **WHEN** the vehicle is predicted to return mid-horizon and a target follows that return
- **THEN** the plan schedules charging in the slots after the return to meet that target

#### Scenario: No charging while away

- **WHEN** the vehicle is predicted to be away for a span of slots
- **THEN** no charging is scheduled in those slots
