# Spec Delta

## Purpose

Lets an experimenter see and switch every VEN's planner objective from the VTN UI, and compare
the fleet's forecast under one objective with its forecast under another, without touching each
VEN individually.

## ADDED Requirements

### Requirement: Each VEN reports its active objective and its source
Every VEN connected to the lab broker SHALL publish, retained, its active planner objective,
where it came from (`profile`, `ven_ui` or `fleet`), when it was set, and the objectives it
accepts, under
`openadr-lab/fleet/<venName>/objective`, on connect and on every change. The VEN UI SHALL show the
same source next to the active objective.

#### Scenario: Objective changed in the VEN UI
- **WHEN** the user switches a VEN's objective on its own Planner page
- **THEN** the VEN publishes the new objective with source `ven_ui`
- **AND** the VTN UI Fleet page shows it within the telemetry freshness window

#### Scenario: Fresh start
- **WHEN** a VEN starts and no fleet command is retained for it
- **THEN** it publishes its profile objective with source `profile`

### Requirement: The VTN UI can switch one VEN or the whole fleet
The VTN UI Fleet page SHALL show each VEN's active objective and source, and SHALL let the user
set the objective of one VEN or of every VEN the fleet view knows. The BFF SHALL turn each such
request into a retained command on `openadr-lab/fleet/<venName>/command/objective` whose body has
the shape of the VEN's `PUT /plan/objective` body plus `requested_at`, the time the user issued
it.

#### Scenario: Switch the whole fleet
- **WHEN** the user picks "opportunistic" for all VENs on the Fleet page
- **THEN** the BFF publishes one retained command per known VEN, one VEN at a time with a
  configurable spacing, so the VENs do not all replan at the same moment
- **AND** each VEN applies it, replans, and reports source `fleet`

#### Scenario: Unknown objective rejected
- **WHEN** the BFF receives an objective request for a VEN with a value that VEN does not list
  among the objectives it accepts
- **THEN** it answers 400 naming the VEN and the value, and publishes nothing

#### Scenario: Command not yet applied
- **WHEN** a command has been published but the VEN has not yet reported the commanded objective
- **THEN** the Fleet page shows that VEN's objective as pending, with the commanded value

### Requirement: The VEN applies fleet commands like a local change
A VEN SHALL apply a received objective command exactly as it applies `PUT /plan/objective`: set
the active objective, trigger an immediate replan, and report the change. The most recent change
SHALL win, whether it came from the fleet command or the VEN UI. A command the VEN has already
applied (same `requested_at`), delivered again because the broker connection was re-established,
SHALL NOT be applied again. A malformed command SHALL be ignored with a warning in the VEN log
and SHALL NOT change the objective.

#### Scenario: Local override after a fleet command
- **WHEN** a fleet command set the objective to opportunistic and the user then picks `min_cost`
  in the VEN UI
- **THEN** the VEN runs `min_cost` and reports source `ven_ui`

#### Scenario: Broker reconnect does not undo a local change
- **WHEN** a fleet command set opportunistic, the user then picked `min_cost` in the VEN UI, and
  the VEN's broker connection drops and comes back
- **THEN** the VEN still runs `min_cost` with source `ven_ui`

#### Scenario: Malformed command
- **WHEN** a command body is not valid JSON or names no known objective
- **THEN** the VEN keeps its active objective and logs a warning naming the topic

### Requirement: A retained fleet command survives a VEN restart
On connecting to the lab broker, a VEN SHALL apply the retained objective command for it, if one
exists, in place of its profile objective. Clearing the command (an empty retained message) SHALL
return a VEN whose active objective came from the fleet to its profile objective with source
`profile`; a VEN whose objective was since set locally keeps it. The VTN UI SHALL offer clearing
per VEN and for the fleet.

#### Scenario: Restart under a fleet command
- **WHEN** a VEN restarts while a retained command sets its objective to opportunistic
- **THEN** its first plan after start-up is opportunistic and it reports source `fleet`

#### Scenario: Releasing fleet control
- **WHEN** the user clears the fleet command for a VEN
- **THEN** the VEN returns to its profile objective, replans, and reports source `profile`

#### Scenario: Releasing does not undo a local choice
- **WHEN** the user clears the fleet command for a VEN whose objective was set in its VEN UI
  after the command
- **THEN** the VEN keeps that objective and its source `ven_ui`

### Requirement: The fleet requests every VEN's forecast over OpenADR
The standing fleet-monitoring event SHALL request a `USAGE_FORECAST` report (`historical: false`,
`readingType: FORECAST`, `frequency: 0`), declared in the program's report payload descriptors
with its unit. A VEN given `frequency: 0` for a forecast report SHALL submit one when it adopts
the first plan after an objective change, and otherwise on adoption at most once per 15 minutes,
carrying one interval per plan slot at the plan's own slot boundaries. Reporting SHALL never
delay or block a planning cycle.

#### Scenario: Forecast report on adoption
- **WHEN** a VEN enrolled in the fleet-monitoring program adopts the first plan after its
  objective changed
- **THEN** it submits a `USAGE_FORECAST` report whose intervals cover the plan's slots
- **AND** the report declares its unit

#### Scenario: Routine replans are throttled
- **WHEN** a VEN adopts periodic plans every 5 minutes without an objective change
- **THEN** it submits at most one forecast report per 15 minutes

#### Scenario: Ad hoc frequency on a measurement report
- **WHEN** a measurement report (`historical: true`) is requested with `frequency: 0`
- **THEN** the VEN reports on its own cadence for that payload type and logs that it applied it

### Requirement: The fleet forecast is the sum of the VENs' forecast reports
The BFF SHALL serve a fleet forecast series: the sum, over every online VEN, of the planned net
power from that VEN's most recent `USAGE_FORECAST` report (import positive, export negative),
read in the quantity and unit the report declares and stated as mean power per slot, aligned to a
common slot grid, with the number of VENs contributing to each slot and the objectives they
planned under (from each VEN's reported objective).

#### Scenario: Forecast changes after a fleet switch
- **WHEN** the whole fleet is switched from `min_cost` to opportunistic
- **THEN** within one planning cycle per VEN, plus the BFF's report polling interval, the fleet
  forecast series reflects the new plans
- **AND** each slot reports which objectives its contributing VENs planned under

#### Scenario: The forecast from before the switch is kept
- **WHEN** a VEN's reported objective changes
- **THEN** the BFF keeps that VEN's newest forecast report as of that moment, and the fleet
  forecast route serves the sum of those reports as a second series, `before`, with the time of
  the change
- **AND** a VEN with no forecast report at the moment of change contributes nothing to `before`
- **AND** the next objective change of that VEN replaces its kept report

#### Scenario: Offline VEN excluded
- **WHEN** a VEN's status is offline
- **THEN** its last forecast is not included in the fleet forecast, and the contributing count
  drops

#### Scenario: Energy declared, power served
- **WHEN** a forecast report declares `KWH` per 15-minute interval with a value of 0.5
- **THEN** the fleet forecast counts that VEN as 2 kW for that slot

### Requirement: Each VEN's adopted plan is inspectable from the VTN UI
Every VEN SHALL publish its adopted plan, retained, under `openadr-lab/fleet/<venName>/plan` on
every adoption, in the shape `GET /plan` serves. Publishing on the lab broker (plan, objective)
SHALL never wait on the broker: an unreachable broker drops the message with a log line and
SHALL NOT delay a planning cycle or a tick. The BFF SHALL keep the latest plan per VEN and
serve it, and the VTN UI Fleet page SHALL show a VEN's planned net power and per-asset
allocations when its row is expanded.

#### Scenario: Drill into one VEN
- **WHEN** the user expands a VEN's row on the Fleet page
- **THEN** the page shows that VEN's adopted plan: objective, net power per slot and each
  asset's allocation per slot

### Requirement: The Fleet power chart overlays forecast on recorded power
The VTN UI Fleet power chart SHALL show the fleet forecast series alongside the recorded fleet
power, distinguishable at a glance, with a legend naming the objectives the forecast is made of.

#### Scenario: Before and after a switch
- **WHEN** the fleet forecast has a `before` series
- **THEN** the chart draws it as a faint dashed line beside the current forecast, labelled with
  the time of the change

#### Scenario: Mixed objectives
- **WHEN** half the fleet runs `min_cost` and half runs opportunistic
- **THEN** the chart legend shows the forecast as made of both objectives with their VEN counts
