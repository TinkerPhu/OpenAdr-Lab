## 1. Shared kit: toggle state hook + legend component

- [ ] 1.1 `charts/useLegendToggle.ts` — a hook returning `{ isHidden(key): boolean,
      toggle(key): void }`, backed by local `useState<Set<string>>`. No persistence, no
      external state source.
- [ ] 1.2 `charts/ChartLegend.tsx` — renders `entries: {key, label, color}[]` as a
      flex-wrapped row; when `interactive` is true, each entry gets a small checkbox
      (`data-testid="legend-toggle-${key}"`) reflecting `!isHidden(key)`, clicking
      checkbox or label calls `toggle(key)`; when `interactive` is false, renders the
      same row with no checkbox (visually a plain legend).
- [ ] 1.3 Unit tests: `useLegendToggle` toggles independently per key, starts empty
      (nothing hidden); `ChartLegend` renders one row per entry, checkbox reflects
      hidden state, interactive=false renders no checkbox elements, clicking a
      checkbox/label calls the toggle callback with the right key.

## 2. TimeSeriesChart: opt-in interactive legend

- [ ] 2.1 Add `interactiveLegend?: boolean` prop. When true: instantiate
      `useLegendToggle`, render `<Legend content={...} />` using `ChartLegend` with one
      entry per `series` item (`key`, `color`), and set `hide={isHidden(s.key)}` on each
      `<Line>`. When false/unset: unchanged — same plain `<Legend>` as before.
- [ ] 2.2 Unit tests: with `interactiveLegend` unset, output is unchanged from the
      existing test suite (regression guard — no assertion changes needed); with it set,
      unchecking a series' entry results in that `<Line>` receiving `hide={true}`, and
      the series' value is absent from a subsequent tooltip-formatter call for that
      hovered point (per spec's "hidden series omitted from tooltip" requirement — verify
      via the `hide` prop, since recharts' own tooltip-payload filtering is a library
      behavior, not something to reimplement/re-test here).

## 3. StackedTimeSeriesChart: unconditional legend grouping + opt-in toggle

- [ ] 3.1 Replace the current `<Legend formatter={...}>` (one row per `${id}_pos`/
      `${id}_neg`, "+"/"-" suffix) with `ChartLegend`, entries built from `assetIds`
      (one per asset, `key=id`, `label=assetLabel(id)`, `color=colorMap[id]`) plus one
      entry for the grid line (`key="grid"`). This grouping applies unconditionally —
      not gated behind `interactiveLegend` — per design.md Decision 1/Goal 2.
- [ ] 3.2 Add `interactiveLegend?: boolean` prop, passed to `ChartLegend`'s `interactive`
      flag. When true: toggling an asset's entry sets `hide={isHidden(id)}` on both its
      `${id}_pos` and `${id}_neg` `<Area>`; toggling "grid" sets `hide` on the grid
      `<Line>`.
- [ ] 3.3 Unit tests: legend renders exactly one entry per asset (not two) regardless of
      `interactiveLegend`; with it enabled, unchecking an asset hides both its pos and
      neg `<Area>` elements; the grid entry toggles independently of any asset.

## 4. Enable on Controller/History consumers

- [ ] 4.1 `AssetTimelineChart.tsx` — pass `interactiveLegend` through from a new prop
      (or hardcode `true`, since every real call site wants it — decide during
      implementation which is less awkward given existing prop surface) so both the
      Controller-tab and History-tab renderings get it.
- [ ] 4.2 `TariffChart.tsx` — same.
- [ ] 4.3 `GridAccumulatedCell.tsx`'s `StackedTimeSeriesChart` usage — same.
- [ ] 4.4 `PlanPowerStack.tsx`'s `StackedTimeSeriesChart` usage — deliberately NOT
      enabled (out of scope per proposal.md), but confirm it still shows the
      one-entry-per-asset grouping fix from task 3.1.
- [ ] 4.5 Raw-diagnostics `TimeSeriesChart` consumers (`TariffsLineChart`,
      `TimelineSeriesChart`), `CurveChart` — deliberately NOT enabled; confirm each is
      visually unchanged (task 2.1's regression guard covers this).

## 5. Documentation

- [ ] 5.1 Update `docs/architecture/chart_diagrams.md` — new "Interactive legend
      (series toggle)" subsection under the relevant compositions; note the
      `StackedTimeSeriesChart` legend-grouping fix in its own composition section.
- [ ] 5.2 Append `docs/history/project_journal.md` entry; add to
      `docs/reference/KEY_LEARNINGS.md` if anything non-obvious surfaces during
      implementation (e.g. any recharts `hide`-prop quirk).

## 6. Verification

- [ ] 6.1 `cd VEN/ui && npm test` — full suite green
- [ ] 6.2 `cd VEN/ui && npm run build`
- [ ] 6.3 ESLint zero errors
- [ ] 6.4 Manual visual + interaction check (needs the user, same limitation as the prior
      chart-primitives change — no browser-automation tool available): Controller tab
      (AssetTimelineChart cells, Grid Tariff, Grid Accumulated), History tab, Planner tab
      (confirm `PlanPowerStack`'s legend shows one row per asset with no checkboxes),
      Raw Diagnostics + Devices (confirm unaffected)
