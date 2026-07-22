## ADDED Requirements

### Requirement: Operator-set PV export ceiling
The simulator SHALL accept an optional PV export ceiling (kW, non-negative
magnitude at the API surface) via the sim-inject mechanism
(`POST /sim/inject`), and SHALL clamp the PV asset's simulated export
power to that ceiling on every subsequent simulator tick until the value
is cleared.

#### Scenario: Setting a ceiling below current natural output curtails PV
- **WHEN** an operator POSTs `pv_export_limit_kw: 3.0` while natural PV
  generation (weather- or sin-model-derived) would otherwise produce more
  than 3.0 kW of export
- **THEN** the simulator's PV asset reports export power clamped to no
  more than 3.0 kW for every tick thereafter, until the limit is cleared
  or raised

#### Scenario: Setting a ceiling above current natural output has no effect
- **WHEN** an operator POSTs `pv_export_limit_kw` set higher than the
  natural PV generation for the current conditions
- **THEN** PV export power is unaffected — it continues to follow natural
  generation (weather or sin-model), since the ceiling is not binding

#### Scenario: Clearing the ceiling restores natural PV output
- **WHEN** an operator POSTs `pv_export_limit_kw: null` (or omits it,
  per the existing sim-inject clear semantics) after a ceiling was
  previously set
- **THEN** PV export power reverts to following natural generation
  unclamped, starting the next tick

#### Scenario: Ceiling persists across ticks without decay
- **WHEN** a `pv_export_limit_kw` ceiling is set and multiple simulator
  ticks elapse without any further sim-inject call
- **THEN** the ceiling remains in effect unchanged on every tick — it does
  not decay back toward an unclamped state the way the `pv_irradiance`
  override does

### Requirement: PV export ceiling changes trigger a replan
Setting or clearing `pv_export_limit_kw` via sim-inject SHALL trigger an
out-of-cycle planner replan, consistent with the existing
`grid_export_limit_kw` sim-inject field's replan-triggering behavior.

#### Scenario: Replan triggered on ceiling change
- **WHEN** an operator POSTs a new `pv_export_limit_kw` value that differs
  from the currently effective value
- **THEN** the planner is triggered to replan before its next scheduled
  periodic cycle, so upcoming plan slots reflect the new PV export
  constraint without waiting up to the full `replan_interval_s`

### Requirement: PV export ceiling is visible in the VEN UI
The currently effective PV export ceiling SHALL be visible in the VEN UI,
both as a live status value and as a settable control.

#### Scenario: Effective ceiling shown on the Dashboard
- **WHEN** a `pv_export_limit_kw` ceiling is currently in effect
- **THEN** the VEN UI Dashboard's PV "Export limit" display shows the
  active ceiling value in kW

#### Scenario: No ceiling shown when unset
- **WHEN** no `pv_export_limit_kw` ceiling is currently set
- **THEN** the VEN UI Dashboard's PV "Export limit" display shows "none"

#### Scenario: Operator can set the ceiling from the Controller tab
- **WHEN** an operator opens the PV asset's controls on the Controller tab
- **THEN** a persistent-override control (not a decaying slider) for the
  export ceiling is available, allowing the operator to set or clear it
  without needing to call the API directly

### Requirement: PV capability reporting is unchanged by curtailment
Introducing the PV export ceiling SHALL NOT change how PV asset
capability is reported to the planner — PV capability continues to report
`max_export_kw == max_import_kw` (fixed), because the planner still
cannot request an arbitrary PV setpoint; it can only be capped from
above by the ceiling.

#### Scenario: PV remains reported as fixed capability with a ceiling active
- **WHEN** a `pv_export_limit_kw` ceiling is currently curtailing PV
  output
- **THEN** the Flexibility & Forecast panel continues to show PV as
  "(fixed)" — the capability range does not widen to reflect the
  ceiling as a controllable range
