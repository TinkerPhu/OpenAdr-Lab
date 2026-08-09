## Context

`PlanPowerStack.tsx` (Planner tab) and `GridAccumulatedCell.tsx` (Controller tab) both
render `StackedTimeSeriesChart` (`components/charts/StackedTimeSeriesChart.tsx`, renamed
from `StackedAreaChart` and relocated out of `controller/charts/` in 045) — the same
component — but feed it from two independently-written data builders. (046, also since
merged, added an opt-in `interactiveLegend` prop to this component; `GridAccumulatedCell`
passes it, `PlanPowerStack` does not — see Decision 5.)

- `GridAccumulatedCell.tsx`'s `buildStackedFromAllTimelines()` reads from
  `useAllTimelines()` → `/timeline/all`, whose backend implementation
  (`controller/timeline.rs`) merges past sim-trace history with future plan-slot data per
  asset, including a synthetic "grid" virtual asset whose `power_kw` is already
  `net_import_kw - net_export_kw` and whose PV series carries `pv_used_kw` (curtailed) plus
  a `pv_forecast_kw` side value.
- `PlanPowerStack.tsx`'s `buildStackedFromPlan()` reads `usePlan()`'s raw `Plan` object
  directly and re-derives the same shape client-side — but for grid power it only reads
  `slot.net_import_kw`, dropping `net_export_kw` entirely. This is the bug.

## Goals / Non-Goals

**Goals**
- Fix the dropped-export bug by removing the second, incorrect implementation rather than
  patching its one line — there should be exactly one place in the frontend that turns
  plan-slot fields into `StackedAreaPoint`s.
- Keep the Planner tab's chart forecast-only (no trailing history), matching current user
  expectation — the only remaining structural difference from the Controller tab's chart
  is `hoursBack: 0` vs `hoursBack: 1`.
- No backend changes: `controller/timeline.rs` is already correct and already does the
  work this change wants the frontend to reuse.

**Non-Goals**
- Do not touch `StackedTimeSeriesChart` rendering, `axisDomain.ts`, the legend-toggle
  primitives, or any other shared chart-kit piece.
- Do not change `net_import_kw`/`net_export_kw` semantics or MILP solver output.
- Do not merge the Planner and Controller pages or make every aspect of their charts
  identical (curtailment banner, page layout stay separate).

## Decisions

### Decision 1: Reuse `buildStackedFromAllTimelines`, don't write a new builder
`buildStackedFromAllTimelines()` already does exactly what `PlanPowerStack` needs and is
already correct (it's the function Controller's chart relies on today). Export it from
`GridAccumulatedCell.tsx` (or relocate it to a shared module if that reads more naturally
once both call sites use it — implementation detail, resolve during coding) and call it
from `PlanPowerStack.tsx`. Do not write a third variant.

### Decision 2: `PlanPowerStack` switches its data source from `usePlan()` to `useAllTimelines()`
`useAllTimelines(hoursBack, hoursForward)` hits `/timeline/all`, a different endpoint from
`usePlan()`'s `/plan`. This adds a network dependency Planner didn't have before (it
already exists on Controller). `usePlan()` is retained for:
- `PlanHeaderBar`, `PlanDecisionMatrix`, `SessionProgressBoard` (unaffected, out of scope).
- The PV-curtailment banner in `PlanPowerStack` itself (`curtailedSlots`, comparing
  `pv_forecast_kw` vs `pv_used_kw` per slot). **Open question for implementation**: the
  timeline's PV series already carries both `pv_forecast_kw` (per-slot value) and
  `power_kw` (= `-pv_used_kw`), so the banner *could* be computed from the timeline
  response instead of a second `Plan` read — decide during implementation whether that's
  worth doing in this change or better left as-is (it isn't the bug being fixed, and
  `usePlan()` stays needed on the page regardless for the header/matrix/session board).

### Decision 3: `hoursForward` for `useAllTimelines` — computed from the plan, not fixed
`PlanPowerStack` currently derives its X-axis window from `plan.slots` directly
(`lastEnd`/`hoursForward` in `buildStackedFromPlan`'s caller). `useAllTimelines` takes an
explicit `hoursForward` argument used both in its query key and as a request param, so this
value must still be computed from `usePlan()`'s data (plan horizon can vary) and passed
into the hook — `usePlan()` isn't eliminated, it's narrowed to header/matrix/session-board
usage plus this one derived window value.

### Decision 4: `hoursBack: 0`
Matches current Planner behavior (forecast-only chart, no trailing history/NOW-line
history tail). This is an explicit, intentional parameter choice, not a default drift —
Controller's `GridAccumulatedCell` passes `hoursBack: 1` (or `EXTENDED_WINDOW`'s value)
for its own reasons unrelated to this fix.

### Decision 5: Do not add `interactiveLegend` to `PlanPowerStack`
046 (merged after this proposal was first drafted) added an opt-in `interactiveLegend`
prop to `StackedTimeSeriesChart`; `GridAccumulatedCell` passes it, `PlanPowerStack` does
not. The repo's `generic-over-bespoke` rule (added to `CLAUDE.md` alongside 046) asks not
to leave near-identical call sites inconsistent without a stated reason — so this is
recorded as a deliberate choice, not an oversight: `PlanPowerStack`'s chart is
forecast-only and single-purpose (verify the plan matches intent), where per-series
toggling has less value than on Controller's longer, denser, multi-window view. Revisit if
a user need for it surfaces; do not add it silently as a side effect of this change, which
is scoped to fixing the grid-power data source, not to UX parity.

## Risks / Trade-offs

- **New query on the Planner page**: `useAllTimelines` adds a polling query
  (`refetchInterval: 10_000` by default) to a page that didn't previously fetch it. Minor
  load increase, consistent with what Controller already does; no mitigation needed beyond
  noting it.
- **Test fixtures**: `__tests__/PlannerPage.test.tsx` mocks `../api/hooks` and does not
  currently stub `useAllTimelines` — the mock module needs to add it, and any
  `PlanPowerStack`-focused test asserting on `buildStackedFromPlan`'s exact output needs to
  move to asserting on `buildStackedFromAllTimelines` + a timeline fixture instead (already
  covered pattern in `GridAccumulatedCell.test.tsx`).

## Open Questions

- Should the PV-curtailment banner also move off `usePlan()` onto the timeline's PV series
  (Decision 2)? Leaning no for this change (smaller diff, banner isn't buggy) — confirm
  during implementation planning (tasks.md) rather than deciding here.
- Does `buildStackedFromAllTimelines` want to move to a shared module (e.g.
  `components/charts/`) now that two pages use it, or is exporting it from
  `GridAccumulatedCell.tsx` fine for two call sites? Lean toward keeping it where it is and
  exporting, unless a third caller appears — avoids an unnecessary module move for its own
  sake.
