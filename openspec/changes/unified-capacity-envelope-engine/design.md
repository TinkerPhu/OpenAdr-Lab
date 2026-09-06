## Context

**Confirmed this session, reading the current code directly:**

- `capacity_forecast.rs::pv_events`'s Import branch computes
  `(-point.planned_kw).max(0.0)` — for currently-generating PV
  (`planned_kw` negative), this yields a positive value, crediting a
  sustained-Import commitment with "curtailing current generation" as if it
  were extra import headroom. The fix (Spec C) is that PV's
  `max_effort_setpoint(Import, tier) == 0.0` always — a trivial constant,
  not a re-derived formula.
- `capacity_forecast.rs::heater_events`'s Export branch uses
  `asset.power_kw` (current draw) instead of `0.0` (heater cannot export at
  all — `capability().max_export_kw` is already pinned to `0.0`). Routing
  Heater through `max_effort_setpoint` fixes this with zero special-casing.
- `envelope_forecast.rs::compute_headroom_forecast` computes `up_kw`/`down_kw`
  as **deltas** from the plan's own `planned_kw`
  (`point.planned_kw - point.cap_max_export_kw`, etc.) — genuinely different
  numbers from an absolute achievable-power quantity, not just a relative
  framing of the same numbers.
- **PV has no time-varying state under Spec C/D's trait-based model.**
  `PvInverter::max_effort_setpoint`/`max_effort_schedule` (Spec C) and
  `resolve_plan_state_at`'s PV handling (Spec D) both return PV's *current
  live* config/state, constant regardless of `t1` or elapsed time — neither
  spec modeled PV's weather-forecast evolution, by deliberate, documented
  design (Spec D's design.md: "no model anywhere in this codebase forecasts
  how [PV's curtailment source] will change"; the same is true of PV's raw
  generation ceiling itself — `PvInverter`'s fields are today's live
  irradiance/weather reading, not a forecast series). The ONLY place a
  time-varying PV ceiling exists is `entities::solar::pv_ceiling_kw`, called
  per-slot by `simulator::forecast::insert_pv_points`
  (`build_forecast_frames`). This is not a new gap this change discovers —
  it is Specs C and D's already-documented scope limit, and this change
  must not silently paper over it by routing PV through primitives that
  would flatten its export ceiling to a constant across a 48-hour sweep
  (a real, visible regression on the Diagnostics chart, not just a subtlety).
- `resolve_plan_state_at` (Spec D) internally recomputes
  `simulated_trajectory` (a full walk across every remaining plan slot) on
  **every call**, extracting one point for the requested `t1`. Calling it
  once per plan slot (which Site Headroom's `t1`-sweep would naturally do)
  costs `O(slots²)` per asset instead of `O(slots)` — `build_forecast_frames`
  itself avoids this by computing the trajectory once and reading every
  slot's point off it. The new engine's Site Headroom path must use the same
  once-per-asset pattern, not repeated `resolve_plan_state_at` calls.
- `CapacityCurve`/`CapacityCurveStep` and `SiteFlexibilityForecastSlot` are
  consumed beyond the two doomed modules: `controller/reporter.rs` and
  `controller/report_intervals.rs` build OpenADR report intervals directly
  from `CapacityCurve`'s steps, and `routes/hems/sessions.rs` /
  `VEN/ui/src/api/types.ts` carry `SiteFlexibilityForecastSlot` through to
  the UI. Both types are kept unchanged in shape — only their *producer*
  changes, and (for `up_kw`/`down_kw`) their *meaning*.

## Goals / Non-Goals

**Goals:**
- One computation, built on Spec C/D's primitives, backing both existing
  consumers as fixed-axis slices of the same `(t1, t2, direction, tier)`
  domain — no new independent per-asset formula for either.
- Fix the two confirmed bugs by construction, not by patching the old
  formulas.
- Site Headroom's numbers become genuinely absolute (plan's own unmodified
  capability at `t1`), matching the master plan's resolved product decision.
- Preserve PV's existing, correct, weather-driven export ceiling — this
  change must not regress PV's forecast accuracy in the name of unification.

**Non-Goals:**
- Materializing the full `(t1, t2)` triangle. Both consumers stay 1-D
  slices (see proposal.md).
- A future-anchored `t1` with its own `t2` sweep and UI control — explicitly
  deferred (master plan's own note).
- Modeling PV's weather forecast inside the `Asset` trait — out of scope for
  Specs C/D, and out of scope here too; PV keeps using the existing
  `pv_ceiling_kw` path for its time-varying Export ceiling.
- Changing `CapacityCurve`/`SiteFlexibilityForecastSlot`'s field shapes —
  `reporter.rs`/`report_intervals.rs`/routes/UI all keep working against the
  same types; only the producing computation changes.

## Decisions

**D1 — PV is asset-kind-and-direction-special, not a full bypass.** PV's
Import contribution (both consumers) uses `max_effort_setpoint(Pv, Import,
tier)` like every other asset — it's already a correct, trivial constant
`0.0` (Spec C), so there's no reason to special-case it away. PV's Export
contribution continues to come from `pv_ceiling_kw` via
`build_forecast_frames`'s existing per-slot resolution — unchanged from
today, because that is the only place a genuinely time-varying PV forecast
exists. This is not new complexity: PV has been the one asset kind treated
specially in every module in this area (`build_forecast_frames`,
`capacity_forecast.rs`, `envelope_forecast.rs`, Spec C, Spec D) — Spec E
continues that established pattern rather than inventing a new one.

- For **Site Headroom** (`t1` sweep, plan slots): PV's Export value at each
  slot is read directly from `build_forecast_frames`'s existing
  `AssetForecastFrame`/`AssetForecastPoint` (`cap_max_export_kw`) — the
  exact same per-slot data `envelope_forecast.rs` already consumes today.
  No new PV computation needed for this consumer at all.
- For **Capacity Forecast** (`t2` sweep from `t1 = now`): **corrected during
  implementation** — the default `plan_horizon_h` is 48 (confirmed:
  `entities/planner_params.rs`), so `build_forecast_frames`'s `pv_frames`
  (already computed once per tick, covering every remaining plan slot)
  already spans exactly the 0–48h range the master plan wants. There is no
  need for a new "call `pv_ceiling_kw` at independent `t2` samples"
  mechanism — PV's Export contribution reuses `pv_frames` verbatim, via the
  *exact same* per-frame delta logic `capacity_forecast.rs::pv_events`'s
  Export branch already uses today (elapsed time = `frame.ts - start`,
  value = `-cap_max_export_kw`). Only the Import branch changes (from the
  buggy formula to the trivial `max_effort_setpoint`-sourced constant
  `0.0`). This is a smaller, safer change than originally planned: PV's
  Capacity Forecast contribution is carried over unchanged except for the
  one line that was actually buggy, on the plan's own natural slot grid —
  not resampled onto an independent uniform `t2` grid.

**D2 — A new primitive for the dense `t2` sweep: `asset_max_power_series`.**
`asset_max_power` (Spec C) returns one `(power_kw, energy_kwh)` pair for one
`t2`. Building a 48-hour capacity curve by calling it independently at many
`t2` samples would mean re-walking the schedule from `t1` to each sample's
end from scratch — `O(samples²)` physics steps for no reason, since
`max_effort_schedule`'s default body already builds a single fine-grained
(60-second-resolution) schedule out to whatever end time is requested.
Instead, `asset_max_power_series(asset, state, t1, t2_max, direction, tier)
-> Vec<(elapsed_s, power_kw, cumulative_energy_kwh)>`
(`assets/max_power.rs`) builds the schedule ONCE out to `t1 + t2_max`, runs
ONE `simulate_forward`, and returns every point — `O(t2_max / 60s)` total
work per asset, not per sample. `asset_max_power` itself is redefined to
call this and read off the last point, guaranteeing the two never diverge
(same "one shared implementation" principle as Spec D's D1).

Every asset kind's `max_effort_schedule` (default body and
`ShiftableLoadAsset`'s override) already steps at the same fixed 60-second
resolution from the same `t1` — so every asset's series lands on the exact
same time grid, letting the site-level curve builder sum them index-wise
with no resampling or interpolation.

**D3 — Dense compute, sparse output.** A 48-hour sweep at 60-second
resolution is ~2,880 points per asset — correct, but a `CapacityCurve` with
2,880 steps would be a real behavior change for `reporter.rs`/
`report_intervals.rs` (one report interval per step today) and unnecessarily
large for the UI. The site-level curve builder sums every asset's series
onto the shared grid, clamps to `[0, cap_kw]`, then **deduplicates
consecutive equal `power_kw` values** into a sparse `CapacityCurveStep` list
— exactly the "ordered by `elapsed_s` ascending" contract `CapacityCurve`
already documents, just produced by scanning a dense simulation instead of
computing exact analytic breakpoints. For simple reservoir exhaustion curves
(the common case), this produces the same small step count as the old
closed-form event math, just via a different (slower, but not
prohibitively so — see Risks) computational route.

**D4 — Site Headroom reuses the trajectory, doesn't repeat it.** Per the
Context section's finding, the new engine does not call
`resolve_plan_state_at` once per slot. Instead it widens
`simulator::forecast::simulated_trajectory`'s visibility (`pub(crate)`) and
calls it once per asset, then — for every trajectory point — calls
`max_effort_setpoint` for both directions to get that slot's absolute
up/down capability. This is architecturally identical to
`insert_simulated_points`'s existing pattern, just reading a different
derived value (`max_effort_setpoint` instead of `capability()`) off the same
per-slot state.

**D5 — `up_kw`/`down_kw` field meaning changes, field names don't.**
`SiteFlexibilityForecastSlot { up_kw, down_kw }` keeps its shape (no
cascading rename across routes/UI types) but its *meaning* changes from
"headroom relative to planned dispatch" to "absolute achievable power":
`up_kw` = absolute max export achievable at this slot
(`CommitmentDirection::Export`), `down_kw` = absolute max import achievable
at this slot (`CommitmentDirection::Import`) — the same direction mapping
`CapacityCurve` already uses. This is a real, user-visible behavior change
(the numbers will generally be larger/different from today), which is
exactly why `SiteHeadroomChart` needs its rendering reworked and why a BDD
scenario must cover the new numbers end-to-end, not just unit tests on the
engine.

**D6 — `SiteHeadroomChart`'s new rendering.** The old band
(`gridPowerKw - up_kw` to `gridPowerKw + down_kw`) only makes sense for a
relative delta. With absolute values, the natural replacement is two lines
(or a band between them) showing the absolute max-export and max-import
ceilings directly, alongside the existing grid-power line — e.g. a band from
`-up_kw` (max export, negative/export-signed) to `down_kw` (max import,
positive/import-signed), which is what "achievable net grid power range"
literally means. Exact visual treatment (band vs. two lines, color) is an
implementation-time UI decision, not pre-specified here — confirm with the
user during that task rather than guessing silently, per this repo's
`tasks.md` convention for open UI questions.

## Risks / Trade-offs

- **[Risk] Dense 60-second-resolution simulation is more compute per tick
  than the old exact closed-form event math** (D3). → **Mitigation:** for a
  48h/60s sweep with a handful of controllable assets, this is on the order
  of 10⁴–10⁵ arithmetic steps per dispatcher tick — small for a single
  Rust-native computation running once every dispatch cycle (seconds to
  minutes apart), not a hot loop. Measure during implementation (task in
  tasks.md) rather than pre-optimizing; if it's a real problem, a coarser
  fixed resolution for the `t2` sweep specifically (not touching Spec C's
  own 60s convention used elsewhere) is the fallback, decided then with
  actual numbers rather than guessed now.
- **[Risk] PV's asset-kind-and-direction special-casing reintroduces the
  "two independent implementations" pattern this whole master plan exists to
  remove** — PV's Export path still doesn't go through `max_effort_setpoint`.
  → **Mitigation:** this is a deliberate, narrow, already-precedented
  exception (D1), not a new instance of the problem: the two confirmed bugs
  this master plan set out to fix are both resolved by construction under
  this design; PV's Export ceiling was never wrong to begin with, so there
  is nothing to unify there — unifying it anyway would require extending
  Specs C/D's scope to model weather forecasting inside the `Asset` trait, a
  materially larger, unrequested piece of work.
- **[Risk] `SiteFlexibilityForecastSlot`'s meaning change is a breaking API
  change for any external consumer of `GET /flexibility/forecast`** (the
  numbers mean something different, even though the JSON shape is
  unchanged). → **Mitigation:** this is a local-network VEN API with no
  external/versioned contract commitment (confirmed: no OpenAPI spec version
  bump convention exists in this repo for this endpoint); the UI is the only
  real consumer, updated in the same change. Document the meaning change
  prominently in the route's own doc comment.

## Migration Plan

1. Add `asset_max_power_series` to `assets/max_power.rs`, redefine
   `asset_max_power` in terms of it (D2).
2. Widen `simulated_trajectory`'s visibility (D4).
3. Build `controller/capacity_envelope.rs`'s two entry points, ported/
   re-verified against `capacity_forecast.rs`'s existing worked-numeric-example
   tests (per this repo's `workflow` rule and the master plan's Verification
   note) plus new tests for the two now-fixed bugs.
4. Wire `tasks/sim_tick/forecast_wiring.rs` to the new module; delete
   `capacity_forecast.rs`/`envelope_forecast.rs`.
5. Update `SiteHeadroomChart.tsx`'s rendering (D6) and its own tests.
6. Add/extend a BDD scenario exercising the absolute-quantity behavior
   end-to-end.
7. Update docs (architecture, use-cases, master plan), delete this change
   directory.

No data migration or rollback concerns — this is a pure computation swap
behind existing, stable API shapes; reverting is a normal git revert if
needed.

## Open Questions

- Exact visual treatment for `SiteHeadroomChart`'s reworked rendering (D6) —
  resolve with the user at that implementation task, not guessed here.
- Whether the Capacity Forecast `t2` sweep's dense-then-sparse computation
  (D3) needs a coarser fixed step for performance — measure during
  implementation, decide with real numbers.
