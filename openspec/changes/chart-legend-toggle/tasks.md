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
- [x] **6.4 Manual visual + interaction check — done on Node1.** Two real issues found
      (wrong checkboxes for dataless Cost-rate/CO₂eq-rate series; redundant color swatch)
      — see the "Correction pass" groups below. Superseded by Group 14's re-check, which
      re-verifies everything together once the corrections land; not re-run standalone.

## 7. TimeSeriesChart core: generic data-presence filtering

- [ ] 7.1 `charts/mergeSeries.ts` — add `seriesHasData(data: TimestampedRow[], dataKey:
      TimeSeriesSeriesSpec["dataKey"]): boolean`, evaluating the accessor (or string key)
      across every row, true if any row yields a non-null value.
- [ ] 7.2 `TimeSeriesChart.tsx` — compute `visibleSeries = series.filter(s =>
      seriesHasData(data, s.dataKey))` once; use `visibleSeries` for both the `<Line>` map
      and `ChartLegend`'s `entries`. Applies regardless of `interactiveLegend`.
- [ ] 7.3 Unit tests: a series with no non-null value anywhere in `data` has no `<Line>`
      and no legend entry (checked both with and without `interactiveLegend`); a series
      that gains a value on a re-render with new `data` appears without any caller-side
      flag; a series whose value is exactly 0 at every row (present, not absent) still
      renders and appears in the legend — locks in the null-vs-zero distinction.

## 8. TimeSeriesChart core: per-series declarative tooltip formatter

- [ ] 8.1 `TimeSeriesSeriesSpec` gains `formatter?: (value: number) => string`. The
      composition's `tooltipFormatter` prop becomes optional (fallback only). Tooltip
      value resolution: look up the hovered series by name in `series`, use its own
      `formatter` if present, else the fallback `tooltipFormatter`, else raw `String(value)`.
- [ ] 8.2 Unit tests: a series with its own `formatter` uses it for its tooltip value; a
      series without one falls back to the chart-level `tooltipFormatter` when provided.

## 9. Migrate AssetTimelineChart onto the generic mechanisms

- [ ] 9.1 Delete `hasNearForecast`/`hasFarForecast`; push the near/far forecast series
      unconditionally (same as Cost rate/CO₂eq rate always were) — task 7's filter hides
      them automatically when the corresponding props are empty.
- [ ] 9.2 Move each series' tooltip formatting (`formatCo2RateGH`, `formatCostRateEurH`,
      `formatSocPct`, `formatTemperatureC`, `formatPowerValue`) from the `tooltipFormatter`
      if-chain into that series' own `formatter` field; delete the if-chain.
- [ ] 9.3 Regression: existing `AssetTimelineChart`/`AssetCell`/`Controller`/`History`
      tests pass unchanged in behavior (update only what the removed booleans/if-chain
      touched, not test intent); new test confirming Cost rate/CO₂eq rate no longer
      appear in the legend for a fixture with no cost/CO2 data.

## 10. Migrate TariffChart onto the generic mechanisms

- [ ] 10.1 Move `formatCo2RateGH`/`formatCostRateEurH`/`formatTariffEurKwh` from the
      `tooltipFormatter` if-chain into each series' own `formatter` field; delete the
      if-chain. (Task 7's data-presence filter applies automatically — no `hasXData` ever
      existed here to remove, since none of the 4 series had one.)
- [ ] 10.2 Regression: existing `TariffChart`/`GridTariffCell`/`History` tests pass.

## 11. Migrate the raw-diagnostics TimeSeriesChart consumers

- [ ] 11.1 `TariffsLineChart.tsx` — move its `if (name === "CO₂ g/kWh") ... else ...`
      tooltip formatter into per-series `formatter` fields; delete the if-chain.
- [ ] 11.2 `TimelineSeriesChart.tsx` — move its (already-trivial, single-series) formatter
      into the series' own `formatter` field, for consistency (no behavior change; it had
      no branch to remove).
- [ ] 11.3 Regression: existing `RawDiagnostics`/`DiagnosticCell` tests pass unchanged.

## 12. StackedTimeSeriesChart: unify Area + legend derivation

- [ ] 12.1 Build one `assetSeries = renderOrder.map(id => ({ id, label: assetLabel(id),
      color: colorMap[id] ?? COLOR_ASSET_FALLBACK }))`; derive the positive-`<Area>` map,
      negative-`<Area>` map, and `ChartLegend` entries all from `assetSeries` instead of
      three independent derivations. Grid stays a separate, hardcoded 4th entry (not part
      of the per-asset family). Deliberately NOT adding data-presence filtering here — see
      design.md's "Additional Non-Goals" (no null/absent signal exists in
      `StackedAreaPoint`'s always-`number` pos/neg fields; confirmed no conflict with the
      unimplemented, unrelated `unify-plan-power-stack-grid` change).
- [ ] 12.2 Regression: existing `StackedTimeSeriesChartLegend`/`GridAccumulatedCell`/
      `PlannerPage` tests pass unchanged; new test confirming an asset's Area elements and
      its legend entry share identical label/color (sourced from the same record).

## 13. Cosmetic: remove the ChartLegend color swatch

- [ ] 13.1 `ChartLegend.tsx` — remove the separate colored square `<span>`; keep the
      (already color-tinted via `accentColor`) checkbox and the colored label text.
- [ ] 13.2 Regression: existing `ChartLegend.test.tsx` assertions still pass (none of them
      assert on the swatch specifically); no new test needed beyond a visual confirmation.

## 14. Documentation & re-verification

- [ ] 14.1 Update `docs/architecture/chart_diagrams.md`: document
      `seriesHasData`/auto-filtering, the per-series `formatter` field, and
      `StackedTimeSeriesChart`'s unified `assetSeries` array.
- [ ] 14.2 Append a `docs/history/project_journal.md` entry for the correction pass; note
      in `docs/reference/KEY_LEARNINGS.md` if anything further surfaces during
      implementation.
- [ ] 14.3 `cd VEN/ui && npm test`, `npm run build`, `npx eslint .` — full green, same
      bar as Group 6.
- [ ] 14.4 Manual visual + interaction check on Node1 (redeploy), replacing the finding
      from 6.4: confirm Cost rate/CO₂eq rate checkboxes no longer appear where there's no
      data, confirm the swatch is gone, re-confirm the original toggle/grouping behavior
      from Group 6 still holds (no regression from this pass).
