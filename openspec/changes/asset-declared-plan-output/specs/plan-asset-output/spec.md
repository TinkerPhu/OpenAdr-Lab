# Spec Delta

## Purpose

Defines what a VEN plan says about each asset it schedules. Every per-asset fact is keyed by the
asset's roster id, and the plan names the assets it planned, so a reader never has to know an
asset's kind to find its part of the plan.

## ADDED Requirements

### Requirement: The plan names the assets it planned
Every plan SHALL carry `planned_asset_ids`: the roster id of every asset the planner scheduled for
that plan, in planner input order, including assets the plan keeps idle in every slot. A fallback
plan (the solve failed) SHALL carry the same list. A plan stored before this field existed SHALL
load with an empty list.

#### Scenario: An idle asset is still named
- **WHEN** the planner plans a battery, an EV and a heater and the battery is idle in every slot
- **THEN** `planned_asset_ids` holds the battery's, the EV's and the heater's ids in input order
- **AND** the battery has no allocation in any slot

#### Scenario: The fallback plan names the same assets
- **WHEN** the solve fails and the fallback plan is returned for the same contexts
- **THEN** its `planned_asset_ids` equal those a solved plan would carry

#### Scenario: Every allocation belongs to a planned asset
- **WHEN** a plan is served by `GET /plan` on a VEN with a battery, an EV and a heater
- **THEN** `planned_asset_ids` names those three assets
- **AND** every allocation's `asset_id` is one of them

#### Scenario: A stored plan without the field still loads
- **WHEN** a plan JSON without `planned_asset_ids` is deserialized
- **THEN** it loads with an empty `planned_asset_ids`

### Requirement: No kind-named per-asset field in the plan
The plan's wire types SHALL carry no field named for one asset kind. A storage asset's charge,
discharge and state of charge are read from that asset's own entries (`allocations`,
`planned_kw_by_asset`, `planned_state_by_asset`); there is no summed battery field.
`baseline_kw`, `pv_forecast_kw` and `pv_used_kw` stay site series until PV and base load take part
in the planner as assets.

#### Scenario: A plan has no battery fields
- **WHEN** a plan is serialized
- **THEN** no slot carries `bat_charge_kw` or `bat_discharge_kw`, and the plan carries no
  `soc_trajectory_kwh`

#### Scenario: The architecture audit refuses a kind-named plan field
- **WHEN** a field such as `bat_charge_kw` is added to a production type in `entities/plan.rs`
- **THEN** `scripts/audit_ven_architecture.py` fails, naming the line

#### Scenario: Plans are otherwise unchanged
- **WHEN** each recorded planner scenario is solved and translated
- **THEN** the model fingerprints and planner inputs match their goldens unchanged
- **AND** the plan golden differs from the one recorded before this change only by the deleted
  battery fields and the added `planned_asset_ids`

### Requirement: The Decision Matrix shows every planned asset
The Planner tab's Decision Matrix SHALL show one row per id in the plan's `planned_asset_ids`,
plus any asset an allocation names that the list lacks (plans stored before the field existed). It
SHALL not name an asset id or kind in its own code.

#### Scenario: An idle planned asset has a row
- **WHEN** the plan's `planned_asset_ids` is `["ev", "heater", "battery"]` and only the EV has
  allocations
- **THEN** the matrix shows a row each for the EV, the heater and the battery

#### Scenario: An older plan still shows its allocated assets
- **WHEN** a plan has no `planned_asset_ids` and allocations for the battery and the EV
- **THEN** the matrix shows a row for the battery and one for the EV
