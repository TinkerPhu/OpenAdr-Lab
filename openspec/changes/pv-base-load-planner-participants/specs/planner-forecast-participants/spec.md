# Spec Delta

## Purpose

Defines how PV and base load take part in the VEN's MILP planner: as assets that declare their
own forecast and their own part of the power balance, like every schedulable asset, while the plan
dispatches only the assets it schedules.

## ADDED Requirements

### Requirement: PV and base load declare their own part of the plan
Each PV and base-load asset SHALL take part in the planner through its own `AssetMilpContext`,
built from its own forecast. A PV asset SHALL declare a used-generation variable bounded by its
forecast, contributing minus that variable to the site balance. A base-load asset SHALL contribute
its forecast as fixed net power. The planner core SHALL name neither kind and SHALL hold no PV or
base-load series of its own.

#### Scenario: Existing profiles plan identically
- **WHEN** each recorded planner scenario is solved and translated
- **THEN** the translated plan matches `plans.txt` byte for byte
- **AND** the model fingerprints differ from their golden only in as-sent digests

#### Scenario: Two PV assets are planned independently
- **WHEN** the planner is given two PV contexts with different forecasts and an export cap below
  their sum
- **THEN** each PV's used generation stays within its own forecast
- **AND** the site's `pv_forecast_kw` and `pv_used_kw` are the sums over both

### Requirement: Forecast participants are not dispatched
An asset's context SHALL declare whether the plan dispatches it or only forecasts it. A forecast
participant SHALL take part in the power balance and the forecast surplus, and SHALL get no
allocation, no `planned_kw_by_asset` entry, no place in `planned_asset_ids` and no share of the
controllable-import malus.

#### Scenario: PV and base load get no allocation
- **WHEN** a plan is made for a site with PV, base load and a battery
- **THEN** only the battery can have allocations and `planned_kw_by_asset` entries
- **AND** `planned_asset_ids` names the battery only

### Requirement: One forecast surplus
The forecast surplus per slot SHALL be computed once, as the forecast participants' generation
minus their load and the baseline override, floored at zero, and every reader (the coexistence
interaction, the controllable-import malus, the EV free-PV cap and the plan's
`surplus_available_kw`) SHALL read that one series.

#### Scenario: Every reader sees the same surplus
- **WHEN** PV forecasts 4 kW and the base load 1 kW in a slot
- **THEN** `forecast_surplus_kw` is 3 kW in that slot
- **AND** the plan's `surplus_available_kw` is 3 kW and the EV free-PV cap is `min(3, p_max)` kW

### Requirement: The plan's PV and base-load fields are site totals
`PlanTimeSlot.pv_forecast_kw`, `pv_used_kw` and `baseline_kw` SHALL be the sums over the forecast
participants (PV forecast, PV used, base load plus the baseline override), so their readers keep
their meaning.

#### Scenario: Curtailment still reaches dispatch
- **WHEN** the plan curtails PV in a slot to relieve an export cap
- **THEN** that slot's `pv_used_kw` is below its `pv_forecast_kw`
- **AND** `resolve_pv_generation_limit_kw` returns the plan's limit for it, as before
