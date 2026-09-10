## ADDED Requirements

### Requirement: PV is the sole authority for its own achievable power
No module outside `VEN/src/assets/pv.rs` SHALL independently compute PV's achievable
export power for any point in time, live or future. Every consumer needing that value
SHALL call into `PvInverter`'s own `Asset` trait methods.

#### Scenario: MILP planning input needs PV's future achievable power
- **WHEN** the MILP planner builds its `p_pv_kw` input array for a future planning slot
- **THEN** it SHALL obtain that value via `PvInverter`'s own trait methods (directly or
  through the shared `asset_max_power_series` primitive), not via a separately-implemented
  formula reading `PvInverter`'s raw snapshot values

#### Scenario: The capacity/headroom engine sweeps a future time point for PV
- **WHEN** `compute_site_headroom_forecast` or the shared site-level capacity-curve
  aggregator evaluates PV's contribution at a future `t1` or `t2`
- **THEN** it SHALL call the same generic per-asset primitive (`asset_max_power_series`/
  `simulated_trajectory`) every other asset kind goes through, not a PV-specific site-level
  computation

### Requirement: External weather data reaches PV via the established per-tick injection channel
Raw external forecast data available to the asset (e.g. a resolved weather-forecast series)
SHALL be supplied to `PvInverter` via the same `TickOverrides`/`TickOverridable` mechanism
already used for `weather_power_kw`, populated once per tick by existing infrastructure —
not via a bespoke parameter threaded through site-level aggregation functions.

#### Scenario: A future weather point is needed for a sustained-commitment sweep
- **WHEN** `PvInverter::max_effort_schedule` is asked for PV's achievable power at a point
  after `t1`
- **THEN** it SHALL read the weather-forecast series from its own state (set via
  `TickOverrides` earlier that tick), sampling it via the existing shared
  `entities::solar::weather_pv_kw_for_slots` function, rather than requiring the caller to
  supply weather data directly

### Requirement: PV's live-instant answer is unchanged by this consolidation
`PvInverter::max_effort_schedule`'s value at `t1` SHALL be identical to
`PvInverter::max_effort_setpoint(Export, Physical)`'s existing, already-correct live answer.

#### Scenario: The seam between live and forecast must match by construction
- **WHEN** `max_effort_schedule`'s first point (`t1`) is compared against
  `max_effort_setpoint`'s answer for the same state
- **THEN** both SHALL compute the value via the same underlying `uncurtailed_power_kw`
  logic, not two independently-written expressions that happen to agree today

### Requirement: A decaying physical quantity is parameterized by its one real time constant
When a value decays exponentially toward a target, the system SHALL store and compute it
from a single time constant (`τ`, in seconds), not a two-parameter encoding (a decay
fraction plus a separately-sourced reference-step duration) where the two parameters only
matter through a derived combination.

#### Scenario: A second copy of the reference-step duration is proposed
- **WHEN** a future change needs to project a `τ`-based decay forward in time from a
  different module than the one that owns the live per-tick update
- **THEN** it SHALL call the owning module's own projection function (e.g.
  `PvSmoothingState::decayed_offset_after`), not re-derive the formula with its own
  independently-sourced reference-step value

#### Scenario: PV's manual-override fade is queried for a future point
- **WHEN** `PvInverter::max_effort_schedule` or `Pv::forecast()` needs the irradiance
  offset's value at a future elapsed time
- **THEN** it SHALL compute `offset × e^(−elapsed_s / τ_s)` via
  `PvSmoothingState::decayed_offset_after`, using the same `τ_s` the live tick's own update
  uses — not a planner-configuration-sourced substitute for `τ`
