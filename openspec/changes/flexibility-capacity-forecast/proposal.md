## Why

`SiteFlexibilityEnvelope` (`up_kw`/`down_kw`, surfaced on `SiteHeadroomChart`) and the per-slot
`SiteFlexibilityForecastSlot` forecast (`envelope_forecast.rs`) both answer "how much power right
now / at this future instant" — neither says how long a given power level is sustainable or how
much energy is behind it. A VTN cannot schedule or size a duration-based DR request (e.g. "hold 5 kW
export for 90 minutes") against either signal, and integrating the existing per-slot forecast over
time is invalid — its own doc comment states each slot is an independent point-in-time counterfactual
("best alternate move here, holding the rest of the plan fixed"), not a conserved multi-slot budget,
so summing it double-counts the same battery kWh at every slot it remains available.

## What Changes

- New closed-form (no MILP re-solve, no forward trajectory simulation) capacity-forecast
  computation, one merged site-level curve per direction (sustained-import-commitment,
  sustained-export-commitment) — power available vs. elapsed time since commitment, plus the
  cumulative energy total up to the first step-down.
- Per-asset-type contribution formulas, computed separately per direction (not mirror images):
  - Battery/EV (SoC-bounded): energy from current SoC to the relevant bound, divided by rated
    power, taperless step down when exhausted; round-trip efficiency applied to the charge
    (import) side only; EV additionally bounded by `soc_target` (not full) and `plugged` state.
  - Heater (thermal-storage-bounded, import-only): energy from `thermal_mass_kwh_per_c ×
    (temp bound − current temp_c)`, divided by the applicable stepped tier (`0`/`mid_kw`/`max_kw`)
    — first-order approximation that ignores ongoing Newton-cooling loss and hot-water draw
    during the commitment window (documented limitation, not corrected in this change).
  - PV curtailment (forecast-bounded, export-only, not a reservoir): "down"-direction (curtail
    current output) integrates the PV forecast curve rather than draining a fixed budget — it
    doesn't step down from exhaustion, it tracks the forecast's own shape; "up"-direction
    contribution is bounded by the same forecast ceiling, not stored energy.
  - Shiftable loads (deadline-bounded, all-or-nothing): fixed energy block (`power_kw ×
    duration_min`) placed once inside `[earliest_start, latest_end]` by a greedy placement rule —
    contributes a single step, not a taper, and is excluded once already dispatched
    (`ShiftableLoadRuntime` present).
  - Base load: confirmed non-flexible (capability mirrors current draw exactly); excluded from
    the curve.
- Per-asset contributions merged into one site-level curve per direction, clipped to the `Grid`
  asset's `import_limit_kw`/`export_limit_kw` (the existing site/interconnection cap — naive
  per-asset power summing can exceed it even though energy summing stays valid), netted against
  concurrent baseline load + PV forecast so the result is net grid power, not raw per-asset
  dispatch.
- OpenADR reporting: expose the resulting flat curve using existing OpenADR 3.1 report payload
  types (no protocol extension) — one interval per future step, payload type chosen from
  `STORAGE_MAX_DISCHARGE_POWER`/`STORAGE_MAX_CHARGE_POWER` or `UP_REGULATION_AVAILABLE`/
  `DOWN_REGULATION_AVAILABLE` with `readingType: FORECAST` (exact choice confirmed during
  `design.md` against how report payload types are already wired in this codebase).
- New VEN UI chart (Diagnostics, not Dashboard) rendering both directions' curves with their step
  points and cumulative energy totals — a new component, not folded into `SiteHeadroomChart`
  (which keeps its current instantaneous-only role).

## Non-Goals

- Ramp-rate / transition-speed limits (how fast the site can reach the committed power) —
  deferred; the curve models steady-state sustainability only.
- Post-commitment recovery/depletion curves (flexibility remaining right after a commitment ends)
  — deferred.
- Uncertainty bands from PV forecast error — deferred; curve is a single deterministic line.
- Advance-notice "curve of curves" (the same curve anchored at multiple future start times) — not
  built as a stored 2D surface; the computation accepts an arbitrary start time so a future
  `reportDescriptor.startInterval`-driven query can reuse it on demand.
- Economic/opportunity-cost pairing (what sustaining this commitment costs against the current
  plan's objective) — deferred.

## Capabilities

### New Capabilities

- `flexibility-capacity-forecast`: closed-form per-direction power/duration/energy capacity curve
  computation, merged across all asset types under the site cap, net of baseline+PV.
- `capacity-forecast-report`: OpenADR 3.1 report wiring that surfaces the capacity curve using
  existing report payload types/reading types, queryable at an arbitrary future start time.
- `capacity-forecast-ui`: new VEN UI Diagnostics chart rendering both directions' curves, step
  points, and cumulative energy totals.

### Modified Capabilities

<!-- No existing openspec specs exist for the affected areas; all capabilities above are net-new. -->

## Impact

- **VEN domain/application layer** (`VEN/src/controller/`): new module(s) for the capacity-curve
  computation (separate file(s) from `envelope_forecast.rs`, which keeps its existing per-slot
  independent-counterfactual contract unchanged); a new port/trait if the computation needs
  asset-type-specific data beyond what `SimulatorPort`/`AssetSnapshot` already expose.
- **VEN entities** (`VEN/src/entities/`): new result type(s) for the per-direction curve (step
  list + cumulative energy), analogous to `SiteFlexibilityForecastSlot`.
- **VEN reporting** (`VEN/src/reporter.rs`, `VEN/src/vtn.rs`): new report payload wiring using an
  existing OpenADR 3.1 enum (no new payload type).
- **VEN routes** (`VEN/src/routes/`): new read endpoint(s) for the UI to fetch the curve, if not
  already served via an existing diagnostics route.
- **VEN UI** (`VEN/ui/src/`): new chart component under Diagnostics; API client/types updates.
- **Grid asset** (`VEN/src/assets/grid.rs`): read-only reference to `import_limit_kw`/
  `export_limit_kw` as the merge cap — no changes to the asset itself.
