## ADDED Requirements

### Requirement: Active import limits are enforced at execution
When limit enforcement is enabled, the arbiter SHALL adjust this tick's setpoints so that projected
net import does not exceed the active hard import limit (the capacity import limit, or 0 kW during
an active alert), using available battery, EV and heater flexibility ranked by marginal cost,
regardless of what the plan scheduled.

#### Scenario: Plan puts a heater stage inside the cap
- **WHEN** the plan schedules a heater at 1.75 kW with 0.47 kW base load under a 1.5 kW capacity limit and the heater's power steps are 0/1.75/3.5 kW
- **THEN** the heater is commanded to 0 kW and projected import is at or below 1.5 kW

#### Scenario: Unforecast load covered by the battery
- **WHEN** base load rises to 2.3 kW under a 1.0 kW capacity limit and the battery can discharge
- **THEN** the battery discharges 1.3 kW and projected import is at or below 1.0 kW

#### Scenario: Planned EV charging reduced
- **WHEN** the plan charges the EV and the capacity limit cannot be met by the battery alone
- **THEN** EV charging is reduced as far as needed

#### Scenario: No plan yet
- **WHEN** no plan has been adopted and a capacity limit is active
- **THEN** the limit is still enforced

### Requirement: Two independent toggles
The arbiter SHALL expose deviation correction (default off) and limit enforcement (default on) as
independent settings; limit enforcement SHALL run whether or not deviation correction is enabled.

#### Scenario: Limit enforcement with deviation correction off
- **WHEN** deviation correction is off and limit enforcement is on
- **THEN** active import limits are enforced and no deviation-from-plan correction is applied

#### Scenario: Limit enforcement off
- **WHEN** limit enforcement is switched off
- **THEN** the arbiter does not adjust setpoints to meet the import limit

### Requirement: One shared target, no opposing corrections
With both toggles on, the deviation correction SHALL aim at the lower of the plan's net and the
hard limit, so that no tick's deviation correction reverses the limit enforcement of the previous
tick.

#### Scenario: Stable over consecutive ticks
- **WHEN** both toggles are on, the plan's net is 2.2 kW and the limit is 1.5 kW, over ten ticks with unchanged conditions
- **THEN** the applied setpoints are identical on every tick after the first and import stays at or below the limit

### Requirement: Heater handling respects stages, thermostat and comfort
The arbiter SHALL command a stepped asset only at one of its reported power steps, SHALL not claim
reduction capacity from an asset whose power is forced by its own thermostat, SHALL restore a paused
stage only with a safety margin below the target, and SHALL curtail heater emergency heat below the
comfort floor only while an alert is active.

#### Scenario: Reduction rounds down to a reachable step
- **WHEN** 0.72 kW must be shed from a heater at 1.75 kW with steps 0/1.75/3.5
- **THEN** the heater is commanded to 0 kW, not to a value the heater would round back up

#### Scenario: Capacity limit does not curtail emergency heat
- **WHEN** the heater's thermostat forces emergency heat under a capacity limit
- **THEN** the emergency heat is not curtailed and any remaining excess is reported as unresolvable

#### Scenario: Alert curtails emergency heat
- **WHEN** the heater's thermostat forces emergency heat during an alert
- **THEN** the heater is put in Curtail mode with setpoint 0

### Requirement: Precedence and bounds
A hard import limit SHALL take precedence over a dispatch setpoint, and limit enforcement SHALL not
exceed per-asset bounds imposed by the comms-loss clamp.

#### Scenario: Dispatch setpoint above the cap
- **WHEN** a DISPATCH_SETPOINT asks for more import than the active capacity limit
- **THEN** import is kept at or below the capacity limit

### Requirement: Decisions are traceable
The arbiter SHALL report each pass's target, excess before and after, adjustments per lever and any
unresolved excess in its diagnostics, and SHALL record a controller event when its decision state
changes (levers used, resolved or unresolved), not on every tick.

#### Scenario: Enforcement starts and ends
- **WHEN** limit enforcement starts reducing load and later stops being needed
- **THEN** exactly one decision event is recorded for each change and the diagnostics show the current state
