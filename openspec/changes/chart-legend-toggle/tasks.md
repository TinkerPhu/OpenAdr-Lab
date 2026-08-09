## 1. Shared kit: toggle state hook + legend component

- [x] 1.1 `charts/useLegendToggle.ts` — a hook returning `{ isHidden(key): boolean,
      toggle(key): void }`, backed by local `useState<Set<string>>`. No persistence, no
      external state source.
- [x] 1.2 `charts/ChartLegend.tsx` — renders `entries: {key, label, color}[]` as a
      flex-wrapped row; when `interactive` is true, each entry gets a small checkbox
      (`data-testid="legend-toggle-${key}"`) reflecting `!isHidden(key)`, clicking
      checkbox or label calls `toggle(key)`; when `interactive` is false, renders the
      same row with no checkbox (visually a plain legend).
- [x] 1.3 Unit tests (`useLegendToggle.test.ts`, `ChartLegend.test.tsx`, 8 tests):
      `useLegendToggle` toggles independently per key, starts empty (nothing hidden);
      `ChartLegend` renders one row per entry, checkbox reflects hidden state,
      interactive=false renders no checkbox elements, clicking a checkbox/label calls
      the toggle callback with the right key.

## 2. TimeSeriesChart: opt-in interactive legend

- [x] 2.1 Added `interactiveLegend?: boolean` prop. When true: instantiates
      `useLegendToggle`, renders `<Legend content={...} />` using `ChartLegend` with one
      entry per `series` item (`key`, `color`), and sets
      `hide={interactiveLegend && isHidden(s.key)}` on each `<Line>`. When false/unset:
      unchanged — same plain `<Legend>` as before (confirmed: `hide` evaluates to the
      literal `false` via short-circuit, never `undefined`).
- [x] 2.2 Unit tests: with `interactiveLegend` unset, all 47 pre-existing
      AssetTimelineChart/TariffChart/RawDiagnostics tests pass with zero edits (the
      regression guard). New `TimeSeriesChart.test.tsx` (4 tests) exercises
      `interactiveLegend`: no checkbox without it, all series start checked/visible with
      it, unchecking a series' real (clicked) checkbox sets `hide={true}` on that
      series' `<Line>` — verified via an actual click-through interaction (recharts
      mocked so `<Legend>` renders its real `content` element), not just static prop
      inspection — re-checking restores it.

## 3. StackedTimeSeriesChart: unconditional legend grouping + opt-in toggle

- [x] 3.1 Replaced the `<Legend formatter={...}>` path (one row per `${id}_pos`/
      `${id}_neg`, "+"/"-" suffix) with `ChartLegend`, entries built from `renderOrder`
      (one per asset, `key=id`, `label=assetLabel(id)`, `color=colorMap[id]`) plus one
      entry for the grid line (`key="grid"`). Applies unconditionally — not gated
      behind `interactiveLegend` — per design.md Decision 1/Goal 2.
- [x] 3.2 Added `interactiveLegend?: boolean` prop, passed to `ChartLegend`'s
      `interactive` flag. When true: toggling an asset's entry sets `hide` on both its
      `${id}_pos` and `${id}_neg` `<Area>`; toggling "grid" sets `hide` on the grid
      `<Line>`.
- [x] 3.3 Unit tests (`StackedTimeSeriesChartLegend.test.tsx`, 4 tests): legend renders
      exactly one entry per asset (not two) regardless of `interactiveLegend`
      (confirmed both with and without it enabled); with it enabled, unchecking an
      asset's real checkbox hides both its pos and neg `<Area>` elements (verified via
      click-through); the grid entry toggles independently of any asset. All 31
      pre-existing StackedAreaTooltip/GridAccumulatedCell/PlannerPage tests pass
      unmodified.

## 4. Enable on Controller/History consumers

- [x] 4.1 `AssetTimelineChart.tsx` — `interactiveLegend` passed as a literal (every real
      call site — Controller cells and History tab both render through this one
      component — wants it, so no new prop threading needed on this file's own props).
- [x] 4.2 `TariffChart.tsx` — same.
- [x] 4.3 `GridAccumulatedCell.tsx`'s `StackedTimeSeriesChart` usage — same.
- [x] 4.4 `PlanPowerStack.tsx`'s `StackedTimeSeriesChart` usage — confirmed NOT enabled
      (`grep` for `interactiveLegend` in the file returns nothing); still shows the
      one-entry-per-asset grouping fix from task 3.1, since that's unconditional.
- [x] 4.5 Raw-diagnostics `TimeSeriesChart` consumers (`TariffsLineChart`,
      `TimelineSeriesChart`), `CurveChart` — confirmed NOT enabled; visually unchanged
      per task 2.2's regression guard (89 tests across all Controller/History/Planner
      consumers pass after enabling, with zero effect on the non-enabled charts).

## 5. Documentation

- [x] 5.1 Updated `docs/architecture/chart_diagrams.md` — new
      `useLegendToggle.ts`/`ChartLegend.tsx` primitives subsection; each composition
      section (`TimeSeriesChart`, `StackedTimeSeriesChart`) now states which real
      consumers have `interactiveLegend` enabled and which deliberately don't;
      `StackedTimeSeriesChart`'s section documents the unconditional legend-grouping fix.
- [x] 5.2 Appended `docs/history/project_journal.md` entry. Added to
      `docs/reference/KEY_LEARNINGS.md`: the recharts-mock technique (mocked `<Legend>`
      renders its own `content` prop instead of returning `null`) that made these tests
      genuinely interactive in jsdom rather than prop-inspection-only.

## 6. Verification

- [x] 6.1 `cd VEN/ui && npm test` — 503/504 green (same pre-existing, network-dependent
      failure noted throughout the prior change; unrelated to this feature)
- [x] 6.2 `cd VEN/ui && npm run build` — succeeds (same pre-existing >500kB chunk-size
      warning, not a new issue)
- [x] 6.3 ESLint zero errors (`npx eslint .` — same 9 pre-existing warnings in unrelated
      files as the prior change's verification, zero errors)
- [ ] **6.4 Manual visual + interaction check — BLOCKED, needs the user** (same
      limitation as the prior chart-primitives change: no browser-automation tool
      available in this environment). Check: Controller tab (AssetTimelineChart cells,
      Grid Tariff, Grid Accumulated — checkboxes appear, clicking one hides/shows that
      series live, unchecked entries are dimmed), History tab (same), Planner tab
      (`PlanPowerStack`'s legend shows one row per asset, no checkboxes), Raw
      Diagnostics + Devices (confirm unaffected — plain legends, no checkboxes). This is
      the one remaining gate before merge.
