## ADDED Requirements

### Requirement: ASAP starts now
Under the opportunistic objective, an active ASAP request SHALL run its asset at full effort from now until
the request's target is reached, taking no surplus on top, whatever the deadline says.

#### Scenario: ASAP EV request
- **WHEN** a plugged-in EV at 30 % holds an ASAP session to 80 % that ends in 12 hours and no PV surplus exists
- **THEN** the EV is commanded its full charge power in the same tick and until it reaches 80 %

### Requirement: BY_DEADLINE takes surplus, then starts as late as the deadline allows
Under the opportunistic objective, an active BY_DEADLINE request SHALL, until its latest start, make its
asset take PV surplus toward the target and nothing from the grid. From the latest start it SHALL run its
asset at full effort until the target is reached. The latest start is the deadline minus the asset's declared
time to its target at full effort, minus a margin. The decision SHALL read no tariff and no forecast.

#### Scenario: Waiting without surplus
- **WHEN** a plugged-in EV at 35 % holds a BY_DEADLINE session to 80 % that ends in 10 hours, needs 3 hours at
  full effort, and no surplus exists
- **THEN** the EV draws nothing for the first 6 hours 45 minutes (the margin is 15 minutes) and runs at full
  power from then until 80 %

#### Scenario: Surplus first
- **WHEN** the same EV sees a 4 kW surplus 2 hours after plug-in
- **THEN** it takes the surplus, its time to target shrinks, and its latest start moves later by the energy
  that surplus delivered

#### Scenario: Once started it keeps running
- **WHEN** the EV has been running at full power since its latest start and a surplus appears
- **THEN** it keeps running at full power until the target is reached and does not stop and restart

#### Scenario: A deadline that cannot be met
- **WHEN** the time left to the deadline is shorter than the asset's time to target
- **THEN** the asset runs at full effort at once

### Requirement: The latest start uses the asset's own time to target
The time an asset needs at full effort to reach the request's target SHALL be declared by the asset on its
snapshot, and the arbiter SHALL read that declaration without converting energy to time itself. An asset
that declares none SHALL be served as if its BY_DEADLINE request were ASAP.

#### Scenario: Asset without a declaration
- **WHEN** an asset holds a BY_DEADLINE target and declares no time to target
- **THEN** it starts now, as an ASAP request does

### Requirement: A plugged-in EV below target is charged without a request
Under the opportunistic objective, an EV that is plugged in, below its target, has no session in force and
has its opportunistic-charging switch on SHALL be served as an ASAP request to its profile target. With the
switch off it SHALL take no part in the pass, as before.

#### Scenario: Plugged in at night
- **WHEN** an EV at 35 % with an 80 % target is plugged in at 20:00 with no session and the switch on
- **THEN** it is commanded full charge power until it reaches 80 %

#### Scenario: Switch off
- **WHEN** the same EV has its switch off
- **THEN** it draws nothing

#### Scenario: Not plugged in
- **WHEN** the EV is away
- **THEN** no request is made for it

### Requirement: The forecast plans the same dispatch
The plan a VEN reports under the opportunistic objective SHALL be produced by the same decision as the live
tick, per slot, so a BY_DEADLINE EV is planned idle until its latest start and then at full power.

#### Scenario: Forecast of a waiting EV
- **WHEN** the plan is built for the EV of the first BY_DEADLINE scenario
- **THEN** its planned power is zero until the latest start and full power from there until 80 %

### Requirement: The pass says what it is waiting for
The outcome of the opportunistic pass (`GET /arbiter-diagnostics`) SHALL list each asset that is waiting for
its latest start with that start's time, and the VEN UI SHALL show it.

#### Scenario: Waiting asset is listed
- **WHEN** the EV of the first scenario is waiting
- **THEN** the diagnostics list it with its latest start, and the VEN UI row for the opportunistic pass shows
  the asset and the time
