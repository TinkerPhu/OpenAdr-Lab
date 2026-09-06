## ADDED Requirements

### Requirement: Capacity Forecast is computed through the unified engine
The system SHALL compute the Diagnostics page's Capacity Forecast (`GET
/flexibility/capacity`) using the shared per-asset extreme-commitment
primitives (`asset_max_power_series`/`max_effort_setpoint`), fixing `t1` at
the current time and sweeping `t2` from 0 to 48 hours, for both Import and
Export directions.

#### Scenario: PV contributes zero to a sustained Import commitment
- **WHEN** the Capacity Forecast is computed for the Import direction while
  PV is actively generating
- **THEN** PV's contribution to every step of the resulting curve is exactly
  `0.0`, not a value derived from PV's current output

#### Scenario: Heater contributes zero to a sustained Export commitment
- **WHEN** the Capacity Forecast is computed for the Export direction while
  the heater is actively drawing power
- **THEN** the heater's contribution to every step of the resulting curve is
  exactly `0.0`, not the heater's current draw

#### Scenario: PV's Export contribution still reflects the weather forecast
- **WHEN** the Capacity Forecast is computed for the Export direction across
  a horizon spanning both day and night
- **THEN** PV's contribution varies across the horizon following the
  weather-driven ceiling (`entities::solar::pv_ceiling_kw`), not a constant
  value held flat from the current instant

### Requirement: Site Headroom reports absolute achievable power
The system SHALL compute the Controller/History Site Headroom forecast
(`GET /flexibility/forecast`) as each asset's absolute achievable power at
each future plan slot (`t2 = 0`, `max_effort_setpoint` evaluated at that
slot's plan-forecasted state), not a delta from the plan's own chosen
dispatch.

#### Scenario: A fully-charged battery reports zero absolute import headroom
- **WHEN** the Site Headroom forecast is computed for a future slot at which
  the plan forecasts a battery to be fully charged
- **THEN** the battery's contribution to that slot's `down_kw` (absolute
  import capability) is `0.0`

#### Scenario: A not-yet-started shiftable load contributes via the same primitive as other assets
- **WHEN** the Site Headroom forecast is computed for a future slot at which
  a shiftable load has not yet started and its window still allows starting
  there
- **THEN** its contribution is computed via the same `max_effort_setpoint`/
  `max_effort_schedule` call every other asset kind gets, not a
  plan-relative "is the currently-chosen slot deferrable" check

### Requirement: The Site Headroom trajectory is computed once per asset, not once per slot
The system SHALL compute each asset's per-slot forecasted state via a single
shared trajectory walk covering all remaining plan slots, reused across
every slot's `max_effort_setpoint` evaluation, rather than re-resolving the
asset's state independently for each requested slot.

#### Scenario: Repeated-computation regression guard
- **WHEN** the Site Headroom forecast is computed for a plan with multiple
  remaining slots
- **THEN** each controllable asset's state-resolving trajectory is computed
  exactly once for the whole forecast, not once per slot

### Requirement: Both consumers share one underlying computation
The system SHALL NOT maintain two independent implementations of "this
asset's achievable power under a commitment" — the Capacity Forecast and
Site Headroom computations SHALL both be expressed as fixed-axis calls into
the same underlying per-asset primitives (`max_effort_setpoint`,
`max_effort_schedule`, `asset_max_power`/`asset_max_power_series`,
`resolve_plan_state_at`/`simulated_trajectory`).

#### Scenario: A fix to the shared primitive is visible in both consumers
- **WHEN** an asset's `max_effort_setpoint` behavior changes
- **THEN** both the Capacity Forecast and the Site Headroom forecast reflect
  the change, without either consumer needing its own separate update
