## MODIFIED Requirements

### Requirement: The Planner tab's Power Stack chart displays grid power sourced from the same computation the Controller tab uses
The Planner tab's Power Stack chart SHALL derive its `StackedAreaPoint[]` chart data —
including grid power — from the same backend-computed timeline data
(`useAllTimelines()` / `/timeline/all`) that the Controller tab's Accumulated Power chart
uses, via the same data-builder function. It SHALL NOT compute grid power by reading only
`net_import_kw` from plan slots, and SHALL NOT maintain a second, independent
plan-to-chart-data builder.

#### Scenario: A slot with export-only grid flow shows a negative grid line
- **WHEN** a plan slot has `net_import_kw` at or near 0 and `net_export_kw > 0` (e.g. under
  an autarky/`min_import` objective with PV surplus)
- **THEN** the Power Stack chart's grid line value for that slot is negative (export),
  matching `net_import_kw - net_export_kw`, not near-zero

#### Scenario: The Planner and Controller tabs agree on grid power for the same plan slot
- **WHEN** the Planner tab's Power Stack chart and the Controller tab's Accumulated Power
  chart both render a time point that falls within the same plan slot
- **THEN** both charts show the same grid power value for that point, differing only in
  which time window each chart is scoped to (Planner: forecast-only from now forward;
  Controller: includes trailing history)

#### Scenario: The Planner tab's chart remains forecast-only
- **WHEN** the Power Stack chart renders
- **THEN** it shows no trailing history before the current time (`hoursBack: 0`), unlike
  the Controller tab's chart which includes a trailing history window

#### Scenario: PV curtailment is still indicated
- **WHEN** the current plan curtails PV in one or more upcoming slots
  (`pv_forecast_kw > pv_used_kw`)
- **THEN** the Planner tab continues to show the curtailment banner with the number of
  affected slots and peak curtailment magnitude
