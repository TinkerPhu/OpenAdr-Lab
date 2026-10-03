# Spec Delta

## Purpose

The EV planner plans for every trip it knows about inside its horizon, so that each departure
has its own readiness target and each return opens a recharge window, making the site's 48-hour
energy forecast reflect all of the vehicle's expected use rather than only its next trip.

## ADDED Requirements

### Requirement: Every known departure inside the horizon binds its own readiness target

The planner SHALL derive one charging obligation per expected departure that falls inside the
planning horizon, whether the departure was stated by a user, queued by the usage simulation, or
predicted by the EV's own usage forecast. An obligation SHALL bind the vehicle's state of charge
at its own departure, independently of every other obligation. A departure beyond the horizon
SHALL contribute no obligation to the current planning cycle.

#### Scenario: Two predicted trips in one horizon each get a target

- **WHEN** an EV declaring a usage forecast with charge planning engaged has two predicted
  departures inside the planning horizon
- **THEN** the plan charges the vehicle to its target before the first departure
- **AND** the plan charges the vehicle to its target again, after the first trip's return and
  before the second departure
- **AND** the planned state-of-charge curve rises, falls at the first return, rises again, and
  falls at the second return

#### Scenario: A departure beyond the horizon binds nothing yet

- **WHEN** an EV's next expected departure falls after the end of the planning horizon
- **THEN** the plan states no readiness obligation for it
- **AND** the plan is feasible and reports no unmet-energy warning for that departure

#### Scenario: A trip the window cannot serve is reported, not refused

- **WHEN** an obligation's target cannot be reached in the slots remaining before its departure
- **THEN** the plan charges everything those slots allow
- **AND** the plan reports the shortfall for that departure specifically, naming it

### Requirement: A return opens a recharge window

The planner SHALL treat the interval between one trip's return and the next trip's departure as
chargeable, and SHALL treat the interval between a departure and its return as not chargeable.
Charging power in a slot the vehicle is expected to be absent for SHALL be zero.

#### Scenario: The gap between two trips is used

- **WHEN** two expected trips are separated by an interval containing cheaper energy than the
  interval before the first trip
- **THEN** the plan places the second trip's charging inside that gap

#### Scenario: No charging while the vehicle is away

- **WHEN** a slot lies entirely within an expected trip's away interval
- **THEN** the plan draws no charging power in that slot, regardless of price or surplus

### Requirement: One derivation serves every producer of expected trips

Stated sessions and predicted trips SHALL yield the planner's availability mask, expected
consumption, and readiness obligations through a single shared derivation. A producer SHALL
contribute only the series of expected trips; it SHALL NOT compute any of those three quantities
itself.

#### Scenario: A stated series and a predicted series of the same shape plan alike

- **WHEN** a user states a series of sessions whose windows, departures, targets and trip
  consumptions match what an EV's usage forecast would predict
- **THEN** the resulting plan is the same as the forecast would have produced
