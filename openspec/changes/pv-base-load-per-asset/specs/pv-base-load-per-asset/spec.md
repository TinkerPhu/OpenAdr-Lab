# Spec Delta

## Purpose

Defines how each PV array and base load gets its own weather forecast, measured feed, preview and
history, so a site with several of them is planned and observed per asset.

## ADDED Requirements

### Requirement: Each PV array has its own weather forecast
A PV asset SHALL carry its own weather geometry, and the VEN SHALL derive each array's power
forecast from that array's geometry and rating.

#### Scenario: East and west arrays
- **WHEN** the roster has an array facing east and one facing west under the same weather forecast
- **THEN** the east array's forecast peaks before solar noon and the west array's after it

#### Scenario: The old site-level section is refused
- **WHEN** a profile still has a top-level `weather_pv:` or `measurements:` section
- **THEN** profile validation fails with a message naming the asset-level place

### Requirement: Measured feeds belong to an asset
A measured PV or base-load feed SHALL feed only the asset that enables it, and at most one PV and
one base-load asset MAY enable one.

#### Scenario: A measured array and an unmeasured one
- **WHEN** one array enables its measurement and a fresh reading arrives
- **THEN** only that array's power follows the reading; the other keeps its weather estimate

### Requirement: Per-asset plan, timeline and forecasts
The plan SHALL keep each forecast asset's own power per slot, and the timeline, forecast
publishing and `GET /weather` SHALL show each PV and base-load asset separately.

#### Scenario: Two arrays in the plan
- **WHEN** a plan is made for two PV arrays
- **THEN** each slot carries each array's own power and the timeline shows two PV series

### Requirement: Base-load learning per asset
The trailing load window, the learned heuristic and the recorder SHALL work per base-load asset.

#### Scenario: A base load named otherwise
- **WHEN** the roster's base load is named `house`
- **THEN** its trailing window and heuristic are learned and applied under `house`
