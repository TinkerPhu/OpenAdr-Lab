## ADDED Requirements

### Requirement: Every asset kind reports its own physical extreme correctly
`Asset::max_effort_setpoint` SHALL return, for a given direction and
`LimitTier::Physical`, exactly the same value `capability(state)` reports for
that direction (`max_import_kw` for Import, `max_export_kw` for Export).

#### Scenario: PV's Import extreme is zero
- **GIVEN** a `PvInverter` in any state
- **WHEN** `max_effort_setpoint` is called with `direction = Import`,
  `tier = Physical`
- **THEN** it SHALL return `0.0`

#### Scenario: Heater's Export extreme is zero
- **GIVEN** a `Heater` in any state
- **WHEN** `max_effort_setpoint` is called with `direction = Export`,
  `tier = Physical`
- **THEN** it SHALL return `0.0`

#### Scenario: Battery's extremes match its charge/discharge ceilings
- **GIVEN** a `Battery` with headroom in both directions
- **WHEN** `max_effort_setpoint` is called for `Import` and separately for
  `Export`, both at `tier = Physical`
- **THEN** the results SHALL equal `capability(state).max_import_kw` and
  `capability(state).max_export_kw` respectively

### Requirement: Shiftable load's extreme is a window-placement decision
`ShiftableLoadAsset::max_effort_schedule` SHALL place the load's run at the
earliest allowed start for `Import` direction, and at the latest allowed
start for `Export` direction, never outside `[earliest_start, latest_end]`.

#### Scenario: Import places the load at its earliest allowed start
- **GIVEN** a pending shiftable load with a window wider than its duration
- **WHEN** `max_effort_schedule` is called with `direction = Import`
- **THEN** the returned schedule SHALL draw `power_kw` starting at
  `earliest_start` (or `t1`, whichever is later) and `0` before and after

#### Scenario: Export places the load at its latest allowed start
- **GIVEN** a pending shiftable load with a window wider than its duration
- **WHEN** `max_effort_schedule` is called with `direction = Export`
- **THEN** the returned schedule SHALL draw `power_kw` starting as late as
  the window allows (`latest_end - duration`, or later-constrained by `t2`)
  and `0` before and after

### Requirement: `assetMaxPower` composes existing primitives with no new simulation logic
`asset_max_power` SHALL produce results equal to manually calling
`max_effort_schedule` followed by `simulate_forward` and integrating the
returned trajectory — it SHALL NOT reimplement any exhaustion/clamping logic
of its own.

#### Scenario: assetMaxPower matches a manual simulate_forward call
- **GIVEN** a `Battery` with a known SoC and a `t1`/`t2` window
- **WHEN** `asset_max_power` is called for `Export` at `tier = Physical`,
  and separately the same schedule is built by hand and passed to
  `simulate_forward`
- **THEN** the power and energy `asset_max_power` returns SHALL match the
  manual `simulate_forward` call's trajectory exactly

### Requirement: `LimitTier` reflects only ceilings the codebase actually has
For asset kinds with no distinct contractual or user-set ceiling below their
physical one, `max_effort_setpoint`/`max_effort_schedule` SHALL return the
same result for `Contractual`, `UserSet`, and `Physical` tiers. For
`PvInverter`, a `Manual`-sourced or `Capacity`/`Plan`/`Arbiter`/`CommsLoss`-
sourced `generation_limit_kw` SHALL clamp the relevant tier's result below
the physical ceiling when active.

#### Scenario: Non-PV asset kinds are tier-invariant today
- **GIVEN** a `Battery` (or EvCharger, Heater, BaseLoad, ShiftableLoadAsset)
  in any state
- **WHEN** `max_effort_setpoint`/`max_effort_schedule` is called at
  `tier = Physical`, `Contractual`, and `UserSet` in turn, same direction
- **THEN** all three results SHALL be identical

#### Scenario: PV's manual curtailment clamps the UserSet tier
- **GIVEN** a `PvInverter` with an active `generation_limit_kw` sourced from
  a manual override, below its physical export ceiling
- **WHEN** `max_effort_setpoint` is called with `direction = Export`,
  `tier = UserSet`
- **THEN** it SHALL return the clamped `generation_limit_kw`, not the
  physical ceiling
