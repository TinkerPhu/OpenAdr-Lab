## Context

Two existing signals already answer "how much power," neither answers "for how long" or "how much
energy":

- `SiteFlexibilityEnvelope` (`up_kw`/`down_kw`) — live, current-instant only.
- `SiteFlexibilityForecastSlot` (`envelope_forecast.rs::compute_headroom_forecast`) — per future
  slot, but each slot is an **independent point-in-time counterfactual** ("best alternate move
  here, holding the rest of the plan fixed"), explicitly not a conserved multi-slot budget. Its own
  doc comment (`envelope_forecast.rs:17-24`) states this is deliberate — summing it across slots
  double-counts the same battery kWh at every slot it remains available. This new capability must
  not extend or reuse that function; it needs its own module with its own contract.

Current per-asset capability primitives already in the codebase (verified by reading, not assumed):

| Asset | Direction(s) | Reservoir? | Existing helper | Gap |
|---|---|---|---|---|
| Battery | both | SoC, `capacity_kwh`, `min_soc` | `AssetConfig::available_storage_kwh` → `((soc-min_soc)*cap, (1-soc)*cap)` | none — correct as-is |
| EV | both | SoC, `battery_kwh`, `min_soc`, **`soc_target`** ceiling (not 1.0) | same `available_storage_kwh`, but computes charge headroom to **1.0**, not `soc_target` | **bug for this use case**: reusing it as-is overstates EV charge (import) capacity — must cap at `soc_target`, matching what `ev.rs::step_inner` actually enforces. Also `plugged=false` → `None` already handled. |
| Heater | import-only | thermal mass, `temp_min_c`/`temp_max_c`/`temp_safety_max_c` band | none — `available_storage_kwh` returns `None` for heater | needs a **new** first-order formula: `thermal_mass_kwh_per_c × (bound − temp_c)`; ignores ongoing `k_loss_kw_per_c` cooling and `draw_kw` during the commitment window (documented approximation, not fixed here). Power is stepped (`0`/`mid_kw`/`max_kw`, `PowerAdjustability::Stepped`), not continuous. |
| PV | export-only | none — forecast-bound, not a reservoir | `capability_inner` gives current ceiling only; forward values need `simulator::forecast::build_forecast_frames` (the same forecast pipeline `envelope_forecast.rs` already consumes) | "down" (curtail) integrates the PV forecast curve itself, never exhausts; "up" bounded by the same forecast ceiling minus current output |
| Base load | none | n/a | `capability_inner` mirrors current draw exactly | confirmed non-flexible — excluded |
| Shiftable load | either, per load | deadline window, not SoC | `ShiftableLoad{power_kw, duration_min, earliest_start, latest_end}` / `ShiftableLoadRuntime` (already-dispatched loads excluded) | all-or-nothing block; needs placement logic, not a per-instant sum |
| Grid | cap only | n/a | `Grid::capability_inner` → `import_limit_kw`/`export_limit_kw` | this **is** the site/interconnection cap the merged curve must clip to |

Existing OpenADR report dispatch (`controller/reporter.rs`) is already payload-type-driven:
`obligation.payload_type` (a plain string set by the VTN's `reportDescriptor`) is matched to decide
which interval-builder runs, and `build_forecast_intervals(active_plan, payload_type)` already
handles the generic "VTN asked for a forecast-shaped payload type" case for `USAGE_FORECAST`. New
capacity payload types slot into this same match arm pattern with their own builder function —
no new report plumbing needed, just a new match arm and a new builder sourcing from the curve
computation instead of `active_plan`.

## Goals / Non-Goals

**Goals:**
- One closed-form (no solver, no forward simulation) merged site-level curve per direction
  (sustained-import-commitment, sustained-export-commitment), each a list of `(elapsed, power_kw)`
  step points plus the cumulative energy total up to the first step.
- Correct, direction-specific, per-asset-type contribution — not a generic "sum SoC" shortcut;
  each asset class (SoC-reservoir, thermal-reservoir, forecast-bound, deadline-bound, non-flexible)
  gets its own formula, computed from state this module already has cheap access to.
- Merge respects the `Grid` asset's `import_limit_kw`/`export_limit_kw` and nets against baseline +
  PV forecast so the result is net grid power.
- Reuse the existing report-dispatch pattern (`payload_type` match in `reporter.rs`) rather than
  inventing new report plumbing.

**Non-Goals:** (unchanged from `proposal.md`) ramp rate, post-commitment recovery, uncertainty
bands, advance-notice curve-of-curves as a stored surface, economic pairing.

## Decisions

1. **New module, not an extension of `envelope_forecast.rs`.** Different contract (conserved
   budget vs. independent counterfactual) and different output shape (step curve vs. per-slot
   scalar) — conflating them risks exactly the double-counting bug the whole feature exists to
   avoid. Proposed location: `VEN/src/controller/capacity_forecast.rs` (new file; keeps both files
   under the 500-production-line cap).

2. **Do not reuse `AssetConfig::available_storage_kwh` unmodified for EV.** It computes charge
   headroom to SoC `1.0`; the asset's own control logic (`ev.rs::step_inner`) never charges past
   `soc_target`. The new module computes EV charge headroom as `(soc_target - soc).max(0.0) *
   battery_kwh` directly, bypassing the shared helper for this one case rather than changing the
   helper's existing (correct-for-its-current-callers) contract.

3. **Heater gets a bespoke formula, not a shared "storage" abstraction — and contributes to BOTH
   directions, not import-only (corrected from an earlier draft of this design).** `available_storage_kwh`
   deliberately returns `None` for heater today — extending it to cover a thermal reservoir would
   change a currently battery/EV-only contract for every other caller. The new module computes
   heater headroom locally: import-direction energy = `thermal_mass_kwh_per_c * (temp_max_c -
   temp_c)`, divided by the applicable stepped tier, stepping down to 0 once exhausted (a genuine
   reservoir-bound term, same shape as battery/EV). Export-direction: the heater never exports, but
   its *current* draw is a reducible baseline — like base load, except flexible down to 0 —
   contributing its current `power_kw` as a constant term for the whole horizon (turning it down
   doesn't consume a stored budget, so no exhaustion step-down on that side). `k_loss_kw_per_c` and
   `draw_kw` are ignored on the import side, and the export side additionally doesn't model the
   forced-on floor re-engaging at `temp_min_c` — both first-order approximations, noted as a Risk
   below, not corrected in this change.

4. **PV contribution is forecast-driven, not reservoir-driven — computed as a running integral,
   not a single energy scalar.** Reuses whatever forecast frames `envelope_forecast.rs` already
   pulls from `simulator::forecast::build_forecast_frames` rather than standing up a second PV
   forecast path. "Down" (curtailable) at elapsed time `t` = current planned/forecast PV output at
   `t`; "up" at `t` = forecast ceiling minus currently-used output at `t`. Because this never
   depletes, PV's curve segment is flat-to-the-forecast-shape, not a taper-to-zero — the merge step
   must treat it as a *time-varying ceiling contribution*, not an energy budget with a
   division-derived duration like the two reservoir-backed asset classes.

5. **Shiftable loads contribute only to the import-commitment (down) curve, as a single
   time-bounded step, not a per-instant sum.** Each not-yet-run, not-currently-running load
   contributes `power_kw` starting at the earliest elapsed time compatible with
   `[earliest_start, latest_end]`, reverting to 0 again after `duration_min` elapses (two events —
   start and end — not one indefinite step; a load runs once for a bounded time), then is removed
   from consideration for the rest of the curve (it cannot be dispatched twice). Reuses
   `envelope_forecast.rs`'s `already_run`/`valid_start_exists_at` (promoted to `pub(crate)`) for
   eligibility.
   **Export-commitment side deliberately not modeled:** `envelope_forecast.rs`'s `shiftable_up_kw`
   lever — deferring a load the *plan* already intends to run imminently, freeing capacity now —
   depends on `Plan.planned_kw_by_asset`. This module otherwise avoids depending on the plan's own
   baseline trajectory (decisions 1–2 above); reintroducing that dependency for shiftable loads
   only, while every other asset class stays plan-independent, would be an inconsistent, easily
   overlooked exception. Left out and documented as a known gap rather than silently included via
   a different mechanism than everything else in this module.

6. **Merge order: sum reservoir/forecast contributions continuously, insert shiftable-load steps
   at their placed elapsed time, then clip the running total to `Grid.import_limit_kw` /
   `Grid.export_limit_kw` at every point.** Clipping the *merged* total (not each asset
   individually) is what prevents the "naive per-asset power summing overstates deliverable power"
   failure mode called out in `proposal.md` — the cap only ever binds the site-level sum.

7. **Base load and PV are net-grid-power contributors, not just flexibility contributors —
   corrected from the original "subtract baseline/PV as a netting pass" framing.** Because every
   asset's term is an *absolute* achievable-power value (not a delta from current dispatch),
   producing genuine net grid power requires adding the non-flexible baseline load as a constant
   (forecasted) term in the same merge, not subtracting it afterward:
   - **Import-commitment curve**: `baseline_load_kw(t)` (site already draws this) is an additive
     term, same sign as every other import contributor.
   - **Export-commitment curve**: `baseline_load_kw(t)` is a *subtractive* term (it keeps consuming
     regardless of the commitment, reducing net export by that amount) — still computed once, in
     the same merge, not as a separate pass.
   - **PV's export-direction contribution is the forecast ceiling itself** (not
     "ceiling minus current output" as originally specified) — the ceiling already represents the
     full achievable export from PV alone (currently-flowing output plus any remaining margin);
     subtracting current output a second time would under-count already-flowing export.
   Base load therefore is NOT excluded from the merge (contradicts the original "Base load
   excluded" requirement — corrected in specs/) even though it contributes zero *flexibility*: it's
   a constant offset both curves must include to represent real net grid power.

8. **Port boundary:** the new module needs per-asset config fields (`capacity_kwh`, `soc_target`,
   `thermal_mass_kwh_per_c`, etc.) that `AssetSnapshot`/`SimulatorPort` do not currently expose
   (today's snapshot only carries `cap_max_import_kw`/`cap_max_export_kw` plus the flattened
   `values` map, not e.g. `soc_target` or `thermal_mass_kwh_per_c` by name). Two options:
   - (a) extend `AssetSnapshot.values` (already a generic `HashMap<String, f64>` sourced from
     `cfg.state_values()`) with the additional fields each asset's `state_values()` needs to emit —
     smallest change, stays inside the existing port shape, no new trait method.
   - (b) add a new `AssetMilpContext`-style trait method (e.g. `capacity_horizon_kwh`) implemented
     per asset type.
   Recommendation: **(a)** — `state_values()` already exists per asset (see `battery.rs:114`,
   which already emits `capacity_kwh`, `max_charge_kw`, `min_soc`) precisely to carry config-plus-state
   through this same flattened-map mechanism; extending it for EV/heater's missing fields (e.g.
   heater's `thermal_mass_kwh_per_c`, `temp_min_c`, `temp_max_c`, `temp_safety_max_c`; EV's
   `soc_target`, `battery_kwh`, `min_soc`) is consistent with the existing pattern and avoids a new
   port. Confirm during implementation whether any field is genuinely missing from `state_values()`
   before adding a new trait method.

9. **OpenADR payload type:** confirm the exact enum(s) during task 1 by checking whether this
   program already issues `reportDescriptor`s of type `STORAGE_MAX_DISCHARGE_POWER` /
   `STORAGE_MAX_CHARGE_POWER` / `UP_REGULATION_AVAILABLE` / `DOWN_REGULATION_AVAILABLE` anywhere
   (VTN UI event/report forms, seed script) — reuse whichever is already wired end-to-end if one
   exists; otherwise add the new match arm(s) to `reporter.rs` following the existing
   `IMPORT_CAPACITY_LIMIT`/`EXPORT_CAPACITY_LIMIT`/`USAGE_FORECAST` pattern (`reporter.rs:132-140`,
   `:283`, `:335`).

## Risks / Trade-offs

- **Heater formula ignores thermal loss and draw during the commitment window** → the reported
  heater contribution will be optimistic (real headroom depletes/replenishes faster than the
  no-loss model assumes near the tail of the window). Mitigation: document as a known
  simplification in the module's doc comment; do not present heater's segment of the curve as
  more precise than "order of magnitude."
- **Reusing `available_storage_kwh` incorrectly for EV would silently overstate charge capacity**
  → mitigated by decision 2 (bypass the helper, compute against `soc_target` directly); add a unit
  test asserting the new module's EV charge headroom stops at `soc_target`, not `1.0`, to guard
  against regressing back to the shared helper later.
- **Greedy shiftable-load placement can disagree with what the MILP planner would actually choose**
  (the planner optimizes cost/comfort across all loads jointly; greedy placement here only asks "is
  a slot available")** → the resulting curve is a *feasibility* bound, not a prediction of what the
  planner would actually do if the commitment were accepted. State this explicitly in the UI and
  in the report payload's implied semantics (this is "could," not "will").
- **PV forecast error propagates into the "down" curve's tail with no uncertainty band** (deferred
  per Non-Goals) → curve should be labeled as deterministic/best-estimate, not a guarantee,
  wherever it's surfaced (UI tooltip, OpenADR payload description if the enum's semantics allow).
- **File-size limit**: the merge logic (7 asset-type formulas + clipping + netting) risks exceeding
  200/500 production lines if kept in one file — plan for a small submodule split (e.g. one file
  for per-asset contribution functions, one for the merge/clip/net pipeline) during task breakdown
  rather than after the audit script flags it.

## Migration Plan

Additive only — new module, new report payload wiring (new match arms, no changes to existing
ones), new UI component, new read route. No existing behavior changes; no data migration; no
rollback beyond reverting the new files/routes.

## Open Questions

- Does any existing `reportDescriptor` in this program already request
  `STORAGE_MAX_DISCHARGE_POWER`/`UP_REGULATION_AVAILABLE`-family payloads (VTN UI, seed script)? If
  yes, reuse that exact enum for continuity; if no, either is a reasonable first choice — confirm
  with the user before locking it in during task 1.
- Should the read route return both directions in one response, or two separate endpoints? Lean
  toward one response (single fetch for the UI chart, matches direction-pairing already used by
  `SiteHeadroomChart`'s up/down band) — confirm no existing route convention says otherwise.
