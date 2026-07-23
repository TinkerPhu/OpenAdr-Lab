## Why

The VEN simulator's `PvInverter.export_limit_kw` field already exists and PV
physics already respects it (`step_inner` and `peek_pv_kw` both clamp output
when it is `Some`), but nothing in the production tick path ever sets it —
it is hardcoded to `None` at construction and only ever set in unit tests.
A separate, *different* mechanism in `controller/dispatcher.rs` computes a
curtailed PV setpoint from the VTN's grid export-limit signal
(`OadrCapacityState.export_limit_kw`), but writes it to the dispatcher's
setpoints map — a channel `PvInverter::step_inner` ignores entirely
(its parameter is literally named `_setpoint_kw`). So both the VTN's
`EXPORT_CAPACITY_LIMIT` signal and any operator override are, today,
completely inert: there is no way to actually curtail PV export in the
simulator. This leaves PV always reported as fixed capability
(`max_export_kw == max_import_kw`) in the Flexibility & Forecast UI panel,
and — most importantly — means the VTN cannot actually enforce an export
capacity limit it has signaled, which is the primary motivation for this
change.

## What Changes

- Add a `pv_export_limit_kw` field to the sim-inject mechanism (mirroring
  the existing `grid_export_limit_kw` pattern) so an operator/UI can set a
  persistent PV export ceiling (kW) at runtime via `POST /sim/inject`.
- Combine that operator override with the VTN's `EXPORT_CAPACITY_LIMIT`
  signal (`OadrCapacityState.export_limit_kw`) into one effective ceiling
  per tick — whichever is more restrictive wins
  (`tasks/sim_tick/tick.rs::effective_pv_export_ceiling_kw`) — and thread
  it into `PvInverter.export_limit_kw` every simulator tick, so PV physics
  (which already clamps on this field) actually curtails from **either**
  source.
- Setting/clearing the operator override triggers an out-of-cycle replan
  (same as `grid_export_limit_kw` today), since a changed export ceiling
  changes what the plan can assume PV will deliver. (VTN capacity changes
  already trigger a replan via their own existing event path.)
- Remove the dead, redundant `capacity.export_limit_kw` → dispatcher
  setpoint clamp in `controller/dispatcher.rs`, since PV physics never
  consumed the setpoint it produced — fully superseded by the mechanism
  above. This also removes the now-unused `capacity` parameter from
  `build_setpoints` and the dead `effective_capacity`/
  `grid_import_limit_kw`/`grid_export_limit_kw` merge in
  `tasks/sim_tick/helpers.rs` that only ever fed that dead clamp.
- Add a third PV `ControlDescriptor` (`pv_export_limit_kw`) so the operator
  override shows up in the generic schema-driven UI controls, using the
  same persistent-override pattern as the heater's `heater_temp_min_c`/
  `heater_temp_max_c` controls (sticks until explicitly cleared — no decay).
- The VEN UI Dashboard's existing "Export limit" display for PV
  (`Dashboard.tsx:326`, already reads `sim.data.assets["pv"].export_limit_kw`
  but has never had real data to show) starts reflecting real values with no
  UI code change needed, since `PvInverter::state_values()` already emits
  the field when set — and will now show whichever of VTN/operator is
  currently binding.

## Non-Goals

- No change to `AssetCapability`/`is_fixed()` semantics for PV — PV
  capability continues to report `max_export_kw == max_import_kw` (fixed).
  The planner still cannot dispatch PV to an arbitrary point within a
  range; it can only be capped from above by this ceiling. Making PV a
  genuine MILP decision variable (planner-optimized curtailment, e.g.
  curtailing for negative export price) is a materially larger effort
  (new `AssetMilpContext` implementation, PV physics setpoint handling,
  envelope/dispatcher-surplus-overlay changes, solver cost terms) and is
  explicitly deferred to a future, separately-scoped change.
- No change to history recording. `TickSample.power_kw` continues to record
  only post-curtailment PV output, same as today. The pre-curtailment
  "available power" value is not separately tracked in history.
- No change to weather-forecast coupling, snow modeling, or the MILP
  planner's PV forecast input path — those already funnel PV forecast
  values through `resolve_weather_pv_kw`/`weather_pv_forecast_series`
  unchanged, and this change does not touch that path except to note (for
  future awareness) that the planner's PV forecast is not itself
  ceiling-aware in this change; only the simulator's live PV output is.

## Capabilities

### New Capabilities
- `pv-export-curtailment`: A runtime-settable PV export ceiling (kW) that
  the simulator enforces in PV physics, exposed via sim-inject and a UI
  control, persistent until explicitly cleared.

### Modified Capabilities
(none — no existing `openspec/specs/` capability covers PV simulator
behavior or sim-inject fields today)

## Impact

- **VEN backend** (Rust): `entities/sim_inject.rs`, `routes/sim.rs`,
  `simulator/mod.rs`, `tasks/sim_tick/tick.rs`, `assets/pv.rs`,
  `controller/dispatcher.rs`.
- **VEN UI** (React/TS): `api/types.ts`,
  `components/controller/AssetRightSection.tsx`.
- No VTN, BFF, or openleadr-rs changes. No OpenADR 3.1 spec constraint —
  this is a simulator-internal operator control, not an OpenADR signal
  path (the existing `EXPORT_CAPACITY_LIMIT` VTN signal path already flows
  through `OadrCapacityState.export_limit_kw` and is untouched by this
  change; it currently reaches only the dead dispatcher clamp being
  removed here, which is a pre-existing gap this change does not attempt
  to close).
