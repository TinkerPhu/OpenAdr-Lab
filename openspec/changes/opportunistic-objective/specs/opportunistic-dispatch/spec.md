# Spec Delta

## Purpose

Defines how a VEN dispatches its assets without look-ahead — the behaviour of a common
non-planning HEMS — so the lab has a baseline to measure planning objectives against, and so a
VEN without a plan does something sensible with every flexible asset.

## ADDED Requirements

### Requirement: Opportunistic is a planner objective
The VEN SHALL offer `opportunistic` as a planner objective alongside the existing ones, settable
through `PUT /plan/objective` and the VEN UI Planner page, and carried as `objective` on every plan
made under it. Selecting it SHALL trigger an immediate replan, as every objective change does.

#### Scenario: Objective selected through the route
- **WHEN** a client sends `PUT /plan/objective` with `{"objective": "opportunistic"}`
- **THEN** the VEN answers 204 and starts a planning cycle
- **AND** the next adopted plan carries `"objective": "opportunistic"`

#### Scenario: Objective selected in the VEN UI
- **WHEN** the user picks the opportunistic objective on the Planner page
- **THEN** the page shows it as the active objective, with source `ven_ui`
- **AND** the next adopted plan carries `"objective": "opportunistic"`

### Requirement: Dispatch ignores plan allocations under the opportunistic objective
While the adopted plan's objective is `opportunistic`, the VEN SHALL NOT dispatch from the plan's
per-slot allocations. Each tick SHALL start from every asset's own default command (including a
thermostat's own setpoint) and apply the opportunistic pass on top. Deviation correction against
a plan target SHALL NOT run.

#### Scenario: Forecast allocations do not drive the assets
- **WHEN** the adopted plan is opportunistic and its current slot allocates 3 kW to the battery
- **AND** the live site balance is exactly zero
- **THEN** the battery is commanded 0 kW, not 3 kW

### Requirement: Surplus is absorbed in the declared surplus-claim order
When the projected site balance is a net export of more than the dead band, the opportunistic
pass SHALL offer the surplus to the assets that can absorb it, in ascending order of each asset
kind's declared surplus-claim rank, each taking what it says it can take toward its own target.
The order SHALL be the same one the MILP planner uses to attribute surplus to assets. An asset
that is full, unplugged, at its comfort ceiling or otherwise unable to absorb SHALL take nothing
and pass the surplus on.

#### Scenario: Scarce surplus follows the order
- **WHEN** 2 kW of surplus is available and the EV and the battery can each take 3 kW
- **AND** the EV's declared rank is lower than the battery's
- **THEN** the EV takes 2 kW and the battery takes 0 kW

#### Scenario: Ample surplus fills every asset
- **WHEN** 10 kW of surplus is available and the EV, the heater and the battery can take 3, 2 and
  4 kW respectively
- **THEN** each takes its full amount and the remaining 1 kW is exported

#### Scenario: A full asset passes the surplus on
- **WHEN** the battery is at its upper SoC limit and 2 kW of surplus is available
- **THEN** the battery takes nothing and the next asset in the order is offered the 2 kW

### Requirement: Import is covered by battery discharge
When the projected site balance is a net import of more than the dead band, the opportunistic
pass SHALL discharge the battery to cover it, down to the battery's own discharge floor. The
battery SHALL NOT discharge more than the import (no export from the battery) and SHALL NOT
charge from the grid.

#### Scenario: Battery covers base load at night
- **WHEN** PV is 0, base load is 0.8 kW and the battery can discharge
- **THEN** the battery discharges 0.8 kW and the site imports approximately 0 kW

#### Scenario: Battery never exports
- **WHEN** the site imports 0.5 kW and the battery could discharge 5 kW
- **THEN** the battery discharges at most 0.5 kW

#### Scenario: Battery never charges from the grid
- **WHEN** the site imports 2 kW and the battery is below its upper SoC limit
- **THEN** the battery is not commanded to charge

#### Scenario: Empty battery leaves the import
- **WHEN** the battery is at its discharge floor and the site imports 1 kW
- **THEN** the battery is commanded 0 kW and the site imports 1 kW

### Requirement: Request modes keep their meaning without look-ahead
Under the opportunistic objective, an active user request SHALL be served according to what its
mode declares for non-planning dispatch: `ASAP` and `BY_DEADLINE` requests SHALL start now at
the asset's full effort toward the request's target; `ASAP_FREE`, `BY_DEADLINE_FREE`,
`OPPORTUNISTIC` and `MAX_COST` requests SHALL be served from surplus only. A request served
from surplus only MAY end unmet at its deadline; the VEN SHALL NOT warn about that as a planning
failure. Surplus left over after start-now requests SHALL still be absorbed in the declared order.

#### Scenario: BY_DEADLINE EV session starts now
- **WHEN** a `BY_DEADLINE` EV session to 80 % is active and the EV is at 40 %
- **THEN** the EV charges at its full rate now, whatever the PV output
- **AND** stops when it reaches 80 %

#### Scenario: OPPORTUNISTIC EV session takes surplus only
- **WHEN** an `OPPORTUNISTIC` EV session is active and there is no PV surplus
- **THEN** the EV is not charged

#### Scenario: Shiftable load in ASAP mode starts at its earliest start
- **WHEN** an `ASAP` shiftable load's earliest start is reached
- **THEN** it starts immediately at its rated power and runs for its duration

#### Scenario: Heater target in BY_DEADLINE mode heats now
- **WHEN** a `BY_DEADLINE` heater target of 55 °C is active and the tank is at 45 °C
- **THEN** the heater heats at full power now and cuts out at 55 °C
- **AND** stays off until the tank has cooled by the heater's thermostat deadband, then reheats,
  as the real thermostat would — it does not switch on and off around 55 °C

### Requirement: The opportunistic surplus toggle keeps its meaning
The existing opportunistic EV charging toggle (`PUT /ev-settings`) SHALL govern whether an EV
with no active request absorbs surplus, under every objective including `opportunistic`.

#### Scenario: Toggle off under the opportunistic objective
- **WHEN** the objective is opportunistic, no EV request is active and the toggle is off
- **AND** there is PV surplus
- **THEN** the EV takes no surplus and the surplus is offered to the next asset in the order

### Requirement: Hard limits still bound opportunistic dispatch
The import-limit enforcement that runs after every tick's dispatch SHALL bound opportunistic
dispatch exactly as it bounds planned dispatch, including start-now requests.

#### Scenario: Start-now EV under a capacity limit
- **WHEN** a `BY_DEADLINE` EV session starts now at 11 kW and a VTN import limit of 5 kW is in
  force
- **THEN** site import stays at or below the limit target and the limit pass reports the lever it
  used

### Requirement: Planning objectives behave exactly as before
Under every objective other than `opportunistic`, the VEN SHALL decide every tick's setpoints and
every plan exactly as it did before this capability existed, given the same inputs and state.
This covers the plan-following tick, the no-plan window (startup and expired plans), the EV
surplus overlay, deviation correction, the limit pass, DISPATCH_SETPOINT and comms-loss handling,
the MILP model and the translated plan. No state the opportunistic objective writes SHALL be read
by a planning objective's decisions.

#### Scenario: Recorded planned ticks are unchanged
- **WHEN** each recorded planned-tick scenario is replayed (plan slot present and absent, EV
  overlay on and off, deviation correction on and off, an import limit, a DISPATCH_SETPOINT window,
  comms loss)
- **THEN** every asset's setpoint and every arbiter outcome field equal the recording made before
  this change

#### Scenario: No-plan window unchanged under a planning objective
- **WHEN** a `min_cost` VEN has no plan yet and the site imports 1 kW with a charged battery
- **THEN** the battery is commanded exactly what it was commanded before this change (0 kW), and
  only the existing EV surplus overlay acts

#### Scenario: Switching back leaves no opportunistic memory
- **WHEN** a VEN ran opportunistic for an hour and is switched to `min_cost`
- **THEN** the residual tracker, the deviation pass's leading lever and the limit pass's leading
  lever hold nothing written by the opportunistic pass
- **AND** from the adoption of the first `min_cost` plan on, the tick decides as a `min_cost` VEN
  in the same physical state would

#### Scenario: A failed solve after switching back still leaves opportunistic dispatch
- **WHEN** a VEN is switched from opportunistic to `min_cost` and the first `min_cost` solve fails
- **THEN** the failed plan replaces the opportunistic plan, and the VEN dispatches as a `min_cost`
  VEN without a usable plan does today
- **AND** it does not keep dispatching opportunistically under a `min_cost` objective

### Requirement: The leading opportunistic lever is visible
The VEN SHALL expose, through its arbiter diagnostics and decision trace, that the opportunistic
pass ran, in which direction, and which asset led it on each tick it acted.

#### Scenario: Diagnostics name the lever
- **WHEN** the opportunistic pass sends surplus to the EV
- **THEN** `/arbiter-diagnostics` reports the opportunistic pass as active with the EV as its
  leading lever
