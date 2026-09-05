## ADDED Requirements

### Requirement: Resolve each asset's forecasted state at a future time
The system SHALL provide a function that, given the live `SimState`, the
active `Plan`, and a target time `t1`, returns each controllable asset's
`AssetState` as forecasted by the plan's own already-decided setpoint
schedule holding from now until `t1`.

#### Scenario: Battery state at a future slot matches its own trajectory
- **WHEN** a battery has an active plan with a decided setpoint schedule and
  the resolver is asked for its state at a future plan slot's start time
- **THEN** the returned `AssetState` equals the state at that same slot from
  a direct `Asset::simulate_forward` call over the same schedule

#### Scenario: EV, heater, and base-load states are resolved the same way
- **WHEN** the resolver is asked for an EV charger's, a heater's, or a
  base-load asset's state at a future plan slot start time
- **THEN** the returned state is produced by the same shared trajectory
  computation used for the battery scenario above, not a separate
  implementation

### Requirement: `t1` at or before now returns the live state exactly
The system SHALL return each asset's current live `AssetState`, with no
simulation, whenever `t1` is at or before the current time.

#### Scenario: t1 equals now
- **WHEN** the resolver is called with `t1` equal to the current time
- **THEN** every returned `AssetState` is identical to that asset's current
  live state in `SimState`

#### Scenario: t1 is in the past
- **WHEN** the resolver is called with a `t1` earlier than the current time
- **THEN** every returned `AssetState` is identical to that asset's current
  live state in `SimState` (no simulation is attempted)

### Requirement: PV's resolved state is its current live state at every `t1`
The system SHALL return the PV inverter's current live `AssetState` for
every requested `t1`, since no model in this codebase forecasts how PV's
curtailment source will change over the horizon.

#### Scenario: PV state at a future t1
- **WHEN** the resolver is asked for the PV inverter's state at a future
  plan slot start time
- **THEN** the returned `AssetState` equals the PV inverter's current live
  state, unchanged

### Requirement: A `t1` beyond the plan's horizon returns the last known forecasted state
The system SHALL return the last available trajectory point's state, rather
than panicking or extrapolating, when `t1` falls after the plan's last
remaining slot.

#### Scenario: t1 past the last slot
- **WHEN** the resolver is called with a `t1` later than the active plan's
  last remaining slot start time
- **THEN** the returned state for each battery/EV/heater/base-load asset
  equals that asset's forecasted state at the plan's last remaining slot
