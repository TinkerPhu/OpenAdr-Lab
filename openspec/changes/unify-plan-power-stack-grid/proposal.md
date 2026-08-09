## Why

Under an autarky (`min_import`) objective, ven-1's Planner tab "Power Stack" chart shows
the black grid line sitting at ~0 kW almost continuously, even in slots where the stack
above it clearly shows a large PV surplus being exported. The Controller tab's
"Accumulated Power" chart — visually near-identical, and since `unified-chart-primitives`
(045) literally the same `StackedTimeSeriesChart` component (`components/charts/`,
renamed from `StackedAreaChart` post-045) — draws the grid line correctly for the same
plan.

Root cause: `PlanPowerStack.tsx` (`VEN/ui/src/components/planner/PlanPowerStack.tsx:29`)
builds its own `StackedAreaPoint[]` directly from `usePlan()`'s raw `Plan` object and sets
`gridPowerKw: slot.net_import_kw`. `net_import_kw` and `net_export_kw` are two separate
non-negative MILP decision variables (`VEN/src/entities/plan.rs:105-107`) — the planner
alternates between them, it never nets them itself. Every other place in the codebase that
turns them into one signed grid-power number does `net_import_kw - net_export_kw`
(`VEN/src/controller/timeline.rs:293`, `report_intervals.rs:36`, `arbiter.rs:134`). Under
autarky, export is unpenalized and common, so most future slots have `net_import_kw ≈ 0`
and a nonzero `net_export_kw` that `PlanPowerStack` silently drops.

The Controller tab's `GridAccumulatedCell.tsx` never has this bug because it doesn't
re-derive grid power from plan fields at all: it reads the `power_kw` of the backend's
"grid" virtual asset from `useAllTimelines()` (`/timeline/all`), which is computed once,
correctly, in `controller/timeline.rs`. `PlanPowerStack` is the only chart-data builder in
`VEN/ui` that reimplements this arithmetic client-side — and the only one that gets it
wrong.

## What Changes

- Fix `PlanPowerStack.tsx` to stop hand-building `StackedAreaPoint[]` from `usePlan()` via
  its own `buildStackedFromPlan()`. Instead, source chart data from `useAllTimelines()` +
  the existing, already-correct `buildStackedFromAllTimelines()` (currently private to
  `GridAccumulatedCell.tsx`; export it for reuse), the same path the Controller tab's
  power-stack cell already uses.
- Delete `buildStackedFromPlan()` and its now-redundant hand-rolled grid/PV/export math.
  `usePlan()` is kept only for the PV-curtailment banner text (`curtailedSlots`, computed
  from `pv_forecast_kw`/`pv_used_kw`) — evaluate during implementation whether that too can
  read off the timeline's PV series (`pv_forecast_kw` is already exposed there per-slot
  alongside `power_kw`, added for exactly this purpose) instead of a second `Plan` read.
- Call `useAllTimelines(hoursBack, hoursForward)` with `hoursBack: 0` so the Planner tab's
  chart keeps its current forecast-only (no trailing history) window — this becomes the
  only remaining intentional difference between the Planner and Controller power-stack
  charts, versus today's two independent (one buggy) data-computation paths.
- No change to `StackedTimeSeriesChart` rendering itself, to the `/timeline/all` backend
  endpoint, or to the MILP planner's `net_import_kw`/`net_export_kw` semantics — this is
  purely a frontend data-sourcing fix on the Planner side.
- `PlanPowerStack` does not adopt 046's `interactiveLegend` prop (see design.md Decision 5)
  — that's a separate, deliberately deferred UX choice, not part of fixing the grid-power
  data bug.

## Non-Goals

- Changing what `net_import_kw`/`net_export_kw` mean or how the MILP solver produces them.
- Merging the Planner and Controller tabs into one page, or making their charts identical
  in every respect (windowing, curtailment banner, and layout stay page-specific).
- Any change to the `unified-chart-primitives` (045) shared chart kit itself — that
  refactor already made the two pages render through the same component; this change fixes
  the data each page feeds into it.

## Capabilities

### New Capabilities
(none)

### Modified Capabilities
- `plan-power-stack`: the Planner tab's Power Stack chart now sources grid/asset power
  from the same backend-computed timeline data the Controller tab uses, instead of
  re-deriving (and, for grid power, mis-deriving) it client-side from the raw `Plan`
  object.

## Impact

- **VEN UI**: `components/planner/PlanPowerStack.tsx` (rewritten to consume
  `useAllTimelines`), `components/controller/GridAccumulatedCell.tsx` (export
  `buildStackedFromAllTimelines` instead of keeping it module-private), `pages/Planner.tsx`
  (no prop-shape change expected, but re-verify).
- **Tests**: `__tests__/PlannerPage.test.tsx` and any `PlanPowerStack`-specific test fixture
  currently asserting on `buildStackedFromPlan`'s output need to assert against
  `buildStackedFromAllTimelines` + a `useAllTimelines` mock instead; add a regression test
  fixture with a slot where `net_export_kw > 0` and `net_import_kw ≈ 0` to lock in the fix
  (the exact shape of the bug this change fixes).
- No backend Rust changes — `controller/timeline.rs`'s existing grid-virtual-asset
  computation is already correct and is not touched.
- No visual change to the Controller tab; the Planner tab's grid line changes from
  (incorrectly) near-zero to correctly negative during export-heavy autarky slots. The
  Planner tab's legend stays non-interactive (unchanged) — see design.md Decision 5.
