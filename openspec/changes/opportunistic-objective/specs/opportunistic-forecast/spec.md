# Spec Delta

## Purpose

Defines the forward-looking plan a VEN publishes under the opportunistic objective, so a fleet's
forecast can be compared between a planning objective and the non-planning baseline at the same
instant and under the same inputs.

## ADDED Requirements

### Requirement: The opportunistic objective publishes a forecast plan
Under the opportunistic objective, every planning cycle SHALL produce a plan covering the
configured horizon and slot grid, with the same fields and meanings as a plan from any other
objective: per-slot tariffs, caps, baseline, PV forecast, PV used, allocations per asset, net
import and export, battery charge and discharge, planned asset state, stored-energy trajectory,
summary and cost breakdown. No MILP solve SHALL run for it.

#### Scenario: Plan served on the existing routes
- **WHEN** the objective is opportunistic and a planning cycle completes
- **THEN** `/plan`, `/forecast` and `/history/plans` serve the new plan in the same shape as a
  MILP plan
- **AND** the plan's `objective` is `opportunistic`

#### Scenario: USAGE_FORECAST report reflects it
- **WHEN** the VTN has requested a `USAGE_FORECAST` report and the objective is opportunistic
- **THEN** the report's intervals carry the opportunistic plan's net power per slot

### Requirement: The forecast is what opportunistic dispatch would do
Each forecast slot SHALL be what the opportunistic dispatch rule decides for that slot, applied
to the slot's forecast PV, forecast base load, forecast tariffs, scheduled capacity limits and
alert windows, starting from the asset states the previous slot ended in. The forecast SHALL be
evaluated once per slot, using the slot's mean PV and base load; it SHALL NOT sub-step within a
slot.

#### Scenario: Forecast and live dispatch agree on the same inputs
- **WHEN** one forecast slot and one live tick are given the same asset states, the same PV,
  base load and limits, and the same active requests
- **THEN** both command the same setpoint for every asset

#### Scenario: Surplus at noon, battery discharge in the evening
- **WHEN** the forecast PV exceeds base load from 10:00 to 16:00 and is zero afterwards
- **THEN** the forecast charges the battery from surplus during the day
- **AND** discharges it to cover base load in the evening until its discharge floor

#### Scenario: Scheduled capacity limit in the horizon
- **WHEN** a VTN import limit of 3 kW is scheduled for a future slot
- **THEN** the forecast's net import in that slot does not exceed the limit target

### Requirement: Forecast slots state the marginal value they assume
Each opportunistic forecast slot SHALL set its marginal cost fields to the tariff the site faces
at the margin in that slot: the import tariff when the slot imports, the export tariff when it
exports, and the import tariff when it is balanced. It SHALL NOT report a shadow price it did not
compute.

#### Scenario: Exporting slot
- **WHEN** a forecast slot exports 2 kW
- **THEN** its marginal cost fields equal the slot's export tariff

### Requirement: Forecast plans are labelled as simulated
A forecast plan SHALL report a solve status that distinguishes it from a solver result, and the
VEN UI SHALL show that status wherever it shows a plan's solve status.

#### Scenario: Status in the plan history
- **WHEN** the user opens the plan history after an opportunistic cycle
- **THEN** that plan is shown with a simulated status, not an optimal one

### Requirement: An opportunistic plan always replaces the plan in force
A plan made under the opportunistic objective SHALL be adopted on every trigger, including
periodic replans, since there is no cost improvement to gate it on. Switching from or to the
opportunistic objective SHALL adopt the new plan immediately.

#### Scenario: Periodic replan
- **WHEN** a periodic planning cycle runs under the opportunistic objective
- **THEN** the new plan is adopted even if its cost is higher than the plan in force

#### Scenario: Switching back to a planning objective
- **WHEN** the objective changes from opportunistic to `min_cost`
- **THEN** the first `min_cost` plan that solves is adopted, and dispatch follows its allocations
  again
