## ADDED Requirements

### Requirement: Per-direction capacity curve computation
The system SHALL compute, on demand for an arbitrary start time, two site-level capacity curves —
one for a sustained-import commitment and one for a sustained-export commitment — each expressed as
an ordered list of `(elapsed_duration, achievable_power_kw)` step points plus the cumulative energy
(kWh) available up to the first step-down. The computation SHALL be closed-form: it SHALL NOT invoke
the MILP solver and SHALL NOT run a forward physics simulation of the committed scenario.

#### Scenario: Two independent directions, not mirrored
- **WHEN** the capacity curve is computed for a given start time
- **THEN** the import-commitment curve and the export-commitment curve are computed from
  independent per-asset formulas (not derived from one another by sign inversion), because the
  contributing asset set and bounds differ per direction (e.g. shiftable loads contribute only to
  the import-commitment curve; battery/EV are symmetric reservoirs but bounded differently per
  direction; heater and base load contribute to both directions via different mechanisms —
  reservoir-bound on heater's import side, constant-term on both's export side)

#### Scenario: Curve is a snapshot, not a stored commitment
- **WHEN** the underlying state (SoC, PV forecast, shiftable-load calendar) changes between two
  computations for the same start time
- **THEN** the two computations MAY produce different curves — the result is not cached or treated
  as binding across ticks

### Requirement: Battery contribution
For a battery asset, the system SHALL compute the sustained-discharge (export-commitment) energy as
`(soc - min_soc).max(0.0) * capacity_kwh` and the sustained-charge (import-commitment) energy as
`(1.0 - soc).max(0.0) * capacity_kwh`, each divided by the asset's rated `max_discharge_kw` /
`max_charge_kw` to produce a duration, with a single step down to zero contribution once that
energy is exhausted. Round-trip efficiency SHALL be applied to the charge (import) direction only.

#### Scenario: Battery at min_soc contributes zero export capacity
- **WHEN** a battery's current SoC equals its `min_soc`
- **THEN** its contribution to the export-commitment curve is 0 kW for the entire horizon

### Requirement: EV contribution bounded by soc_target, not full charge
For an EV asset, the system SHALL compute sustained-charge (import-commitment) energy as
`(soc_target - soc).max(0.0) * battery_kwh` — bounded by the asset's configured `soc_target`
ceiling, not by 100% SoC — and sustained-discharge (export-commitment) energy as
`(soc - min_soc).max(0.0) * battery_kwh`. An EV with `plugged = false` SHALL contribute zero to
both directions.

#### Scenario: EV charge headroom stops at soc_target
- **WHEN** an EV's current SoC is below both `soc_target` and 1.0
- **THEN** the computed import-commitment energy for that EV equals
  `(soc_target - soc) * battery_kwh`, not `(1.0 - soc) * battery_kwh`

#### Scenario: Unplugged EV contributes nothing
- **WHEN** an EV's `plugged` state is false
- **THEN** its contribution to both the import-commitment and export-commitment curves is 0 kW

### Requirement: Heater contribution — both directions, asymmetric formulas
A heater never exports power itself, but its *current* draw is a reducible baseline (like base
load, except flexible down to 0) — the system SHALL therefore compute a contribution on both
curves:
- **Import-commitment** (increase heating): energy = `thermal_mass_kwh_per_c * (temp_max_c -
  temp_c)`, divided by the applicable stepped power tier (`0` / `mid_kw` / `max_kw`) above the
  current tier to produce a duration, stepping down to 0 once exhausted.
- **Export-commitment** (decrease/turn off heating, freeing consumption): contributes the heater's
  *current* `power_kw` as a constant term for the whole horizon (no exhaustion step-down) —
  turning the heater down doesn't consume a stored energy budget the way charging does.

Both formulas SHALL NOT account for ongoing thermal loss (`k_loss_kw_per_c`) or hot-water draw
(`draw_kw`) during the commitment window; the export-direction term additionally SHALL NOT model
the forced-on floor at `temp_min_c` re-engaging after sustained reduction (both are documented
simplifications, not corrected in this change).

#### Scenario: Heater at temp_max_c contributes zero further import headroom
- **WHEN** a heater's current temperature equals `temp_max_c`
- **THEN** its contribution to the import-commitment curve is 0 kW

#### Scenario: Heater's current draw contributes to export-commitment
- **WHEN** a heater is currently running at `mid_kw`
- **THEN** its contribution to the export-commitment curve is `mid_kw`, held constant for the
  whole curve (not stepping down)

### Requirement: PV contribution is forecast-bound, not energy-bound
For a PV asset, the system SHALL compute its export-commitment contribution at each future elapsed
time `t` as the PV forecast ceiling at `t` (the full achievable export from PV alone — already
includes currently-flowing output plus any remaining margin, so it SHALL NOT additionally subtract
current output), and its import-commitment "down" contribution (curtailment) at `t` as the
forecast/planned PV output at `t`. PV's contribution SHALL be sourced from the same forecast-frame
mechanism already used by `envelope_forecast.rs` (`simulator::forecast::build_forecast_frames`).
PV's contribution SHALL NOT be modeled as a depleting energy budget and SHALL NOT produce a
step-down-to-zero shape from exhaustion.

#### Scenario: PV contribution tracks the forecast shape over the horizon
- **WHEN** the PV forecast predicts declining irradiance later in the commitment window
- **THEN** PV's contribution to the curve decreases following the forecast, not because a fixed
  energy budget was exhausted

### Requirement: Shiftable loads placed once, on the import-commitment curve only
For each shiftable load not yet run (no matching `ShiftableLoadRuntime`) and not currently running,
the system SHALL place its full `power_kw` on the **import-commitment curve only** as a single
time-bounded step — active from the earliest elapsed time within `[earliest_start, latest_end]`
until `duration_min` later, reverting to 0 kW after that — and SHALL exclude that load from further
placement within the curve. A load already running or already completed SHALL contribute nothing.
Shiftable loads SHALL NOT contribute to the export-commitment curve (deferring a load the plan
currently intends to run is out of scope for this closed-form computation — see design.md decision
5).

#### Scenario: A shiftable load contributes at most once, for a bounded duration
- **WHEN** a shiftable load's valid window spans multiple elapsed points on the curve
- **THEN** its `power_kw` appears as a single step starting at one elapsed time and ending
  `duration_min` later, not summed at every valid point within its window and not held
  indefinitely

#### Scenario: Already-dispatched load excluded
- **WHEN** a `ShiftableLoadRuntime` exists for a load
- **THEN** that load contributes 0 kW to both direction curves for the remainder of its life

#### Scenario: Shiftable loads absent from the export-commitment curve
- **WHEN** the export-commitment curve is computed
- **THEN** no shiftable-load term is included in its merge, regardless of eligibility

### Requirement: Base load is a constant net-grid-power term, not a flexibility contributor
Base load SHALL NOT contribute to either curve's *flexibility* (it never changes with the
commitment), but its forecasted power SHALL be included as a constant additive term on the
import-commitment curve and a constant subtractive term on the export-commitment curve, so that
each curve represents genuine net grid power rather than only the flexible assets' own dispatch.

#### Scenario: Base load raises the import-commitment floor
- **WHEN** the import-commitment curve is computed and base load is forecast to draw 0.5 kW
  throughout the horizon
- **THEN** every step's value is at least 0.5 kW higher than it would be with base load at 0 kW

#### Scenario: Base load lowers the export-commitment ceiling
- **WHEN** the export-commitment curve is computed and base load is forecast to draw 0.5 kW
  throughout the horizon
- **THEN** every step's value is 0.5 kW lower than it would be with base load at 0 kW (before
  clipping to the site cap)

### Requirement: Site cap clipping on the merged total
The system SHALL sum all per-asset continuous contributions (including base load's constant term),
plus placed shiftable-load steps, into one running total per direction, then clip that running
total (not each individual term) to the `Grid` asset's `import_limit_kw` (import-commitment curve)
or `export_limit_kw` (export-commitment curve) at every elapsed point.

#### Scenario: Combined asset power exceeds the site cap
- **WHEN** the sum of individual contributions at some elapsed time exceeds the site's
  `import_limit_kw`
- **THEN** the merged curve's value at that elapsed time is clipped to `import_limit_kw`, not the
  uncapped sum
