> **Review checkpoint (after tasks 1-5, 9):** a code review of the branch surfaced 5 issues,
> all fixed in a follow-up commit: (1) `TariffChart`'s tariff-axis domain used
> `minSpanDomain` (0-anchored), which reintroduced axis-squeeze for always-positive tariff
> series — added `tightSpanDomain()` (data-tight, no 0-anchor) and switched tariff to it;
> (2) `StackedAreaChart`'s signed-power tooltip could silently drop the sign on a sub-watt
> negative residual — fixed and moved to a shared `formatSignedPowerValue` in
> `unitFormat.ts`; (3) `unitFormat.ts`'s newer formatters used bare `value` params,
> violating the project's unit-suffix naming rule — renamed; (4) `ComfortCurveChart`'s
> tooltip still hand-rolled tariff formatting instead of using the new canonical
> formatter — switched; (5) `GridAccumulatedCell.tsx` duplicated the signed-power pattern —
> now shares `formatSignedPowerValue` too. All fixes covered by new regression tests.

## 1. Shared kit: sizing contract and color registry

- [x] 1.1 Create `VEN/ui/src/components/charts/` (resolves design.md's open module-path
      question — top-level, since raw-diagnostics already imports across folders from
      `controller/`); move `CELL_CHART_HEIGHT`/`CELL_CHART_HEIGHT_TALL`/window/tick-interval
      constants from `controller/chartLayout.ts` into `charts/chartLayout.ts`, adding
      `DIAGNOSTIC_CHART_HEIGHT = 260` alongside them
- [x] 1.2 Extend `controller/types.ts`'s `ASSET_COLORS` (or move it into
      `charts/colorRegistry.ts`) with named keys `import_tariff`, `export_tariff`,
      `cost_rate`, `co2_rate`, `grid_line`, using the existing hex values currently
      hardcoded as `COLOR_IMPORT_TARIFF`/`COLOR_EXPORT_TARIFF`/`COLOR_COST_RATE`/
      `COLOR_CO2_RATE` (`TariffChart.tsx`) and `COLOR_GRID_LINE` (`StackedAreaChart.tsx`) —
      landed as a new `SERIES_COLORS` map alongside `ASSET_COLORS` in `types.ts` (kept
      in place rather than a new `colorRegistry.ts` file, to avoid an unnecessary extra
      module for six keys); also added a `power` key for the raw-diagnostics generic
      single-series charts (task 1.3), not in the original list
- [x] 1.3 Delete `raw-diagnostics/colors.ts` (`CHART_COLORS`) once no importer remains —
      done directly (not deferred to task 6): `SimProfileChart`/`TariffsLineChart`/
      `TimelineSeriesChart` now consume `SERIES_COLORS` directly; this is an intentional,
      disclosed recolor of import/export tariff and CO₂ on the Raw Diagnostics page per
      design.md Decision 3
- [x] 1.4 Unit tests: registry returns the same color for a given key regardless of caller;
      sizing constants exported and distinct (`DIAGNOSTIC_CHART_HEIGHT !== CELL_CHART_HEIGHT`)
      — `__tests__/chartLayout.test.ts`

## 2. Shared kit: axis-domain and tick engine

- [x] 2.1 Move `axisDomain.ts` from `controller/charts/` to `charts/axisDomain.ts`
      (re-export or update all current importers: `AssetTimelineChart`, `StackedAreaChart`,
      `SimProfileChart`, `TimelineSeriesChart`)
- [x] 2.2 Add `MIN_COST_RATE_SPAN_EUR_H`-style floor constant for tariff (€/kWh) — currently
      `TariffChart`'s primary axis has no floor at all — landed as `MIN_TARIFF_SPAN_EUR_KWH`
- [x] 2.3 Add zero-anchored tick generation: when a resolved domain has `min < 0 < max`,
      generate ticks as `{0, ±s, ±2s, ...} ∩ [min, max]` instead of the current
      start-anchored generation — `zeroAnchoredTicks()`; wired into every mixed-sign axis
      found (`AssetTimelineChart` power/cost/co2, `StackedAreaChart` power, `TariffChart`
      tariff/cost/co2), not deferred to the composition-migration tasks
- [x] 2.4 Unit tests: a mixed-sign domain (e.g. `[-3, 5]`) always includes `0` in the
      returned tick set; ticks are symmetric steps from 0; a same-sign domain's ticks are
      unchanged from current output (regression guard); tariff axis floor behaves like the
      existing power/cost/CO₂ floors — `__tests__/axisDomain.test.ts`

## 3. Shared kit: canonical per-unit value formatting

- [x] 3.1 Add a single formatter module (`charts/unitFormat.ts`) implementing the spec's
      per-unit table: power (magnitude-aware W/kW, extending `formatPowerTick` to cover
      tooltips too), cost €/h (4dp), CO₂ rate g/h (1dp), CO₂ intensity g/kWh (3dp, kept
      distinct from CO₂ rate), tariff €/kWh (4dp), SoC % (1dp), temperature °C (1dp) —
      each returning both the formatted number and its unit label. Wired into every
      tooltip found: `AssetTimelineChart`, `TariffChart`, `StackedAreaChart` (via a local
      `signedPower` wrapper preserving its existing "+/-" sign convention),
      `SimProfileChart`, `TimelineSeriesChart`, `TariffsLineChart` (which previously
      formatted CO₂ intensity with the same rule as tariff — now split correctly). Also
      wired all three raw-diagnostics charts onto `DIAGNOSTIC_CHART_HEIGHT` (was a bare
      `260` literal in each file) while touching these files, ahead of task 6.4.
- [x] 3.2 Unit tests: one test per unit rule from the spec's table, including the
      boundary case at exactly 1 kW (Watts below, kW at-and-above); a value's unit label
      is never a bare `€` for €/kWh or €/h series — `__tests__/unitFormat.test.ts`

## 4. Shared kit: data-merge builder (the cursor-correctness fix)

- [x] 4.1 Extract `AssetTimelineChart`'s existing timestamp-merge-with-LOCF logic
      (the `117b44f` fix) into `charts/mergeSeries.ts` — a function taking N per-series
      arrays and producing one timestamp-keyed row array, forward-filling sparse series.
      `AssetTimelineChart.tsx` itself refactored to call the shared `mergeTimestampedSeries`/
      `locfFillKeys` instead of its private inline versions — its existing test suite
      (55 tests across `AssetTimelineChart`/`AssetCell`/`Controller`/`History`) passes
      unchanged, confirming the extraction preserved behavior exactly.
- [x] 4.2 Design the composition components (task 6/7) so `<Line>`/`<Area>` rendering only
      ever accepts a `dataKey` into the merged array — no prop path exists for passing an
      independent per-series `data` array. (Contract established now via `mergeSeries.ts`'s
      shape; enforced in the composition components themselves in tasks 6–7.)
- [x] 4.3 Add a shared test helper (`charts/testUtils/assertTooltipMatchesData.ts`) that,
      given a hovered timestamp, asserts each series' accessor-returned value equals the
      merged row's own value for that series at that timestamp
- [x] 4.4 Unit tests (`__tests__/mergeSeries.test.ts`): two series sampled at different
      rates (1-min vs 5-min) merge into one array with correct (non-cross-contaminated)
      values at each timestamp; the test helper is exercised two ways — passes against a
      correct accessor reading the merged row, and (written first, confirmed failing
      against the old per-series-array pattern before the helper existed) throws when
      given a deliberately-reintroduced accessor that reads an independent, misaligned
      array by position instead of by timestamp — proving the helper actually catches the
      `117b44f`/`f7b911e` bug class rather than being a tautology

## 5. Shared kit: NOW line, zone shading, tooltip style, empty state

- [x] 5.1 Extract the NOW `<ReferenceLine>` block (color, dash pattern, label) duplicated
      in `AssetTimelineChart`/`StackedAreaChart`/`TariffChart` into `charts/NowLine.tsx`.
      Implemented as a `renderNowLine()` function returning the element directly (not a
      wrapping component) — recharts inspects its direct children's types to compute axis
      domains/positioning, so an intermediate custom component type would risk changing
      what recharts sees at that position in the tree; a function call spliced as
      `{renderNowLine(...)}` produces the identical element, avoiding that risk entirely.
- [x] 5.2 Extract the `zones?.map(...)` `<ReferenceArea>` block into `charts/ZoneShading.tsx`
      — same function-not-component approach as 5.1, for the same reason.
- [x] 5.3 Extract tooltip container styling (`contentStyle`/`itemStyle`/`labelStyle`) into
      `charts/tooltipStyle.ts`, consumed by both the declarative `<Tooltip>` prop path and
      `StackedAreaTooltip`'s custom JSX (added `TOOLTIP_BOX_STYLE`, replicating recharts'
      own default tooltip box chrome for the custom-JSX path)
- [x] 5.4 Extract one empty-state component (`charts/EmptyState.tsx`) — scoped narrower
      than originally planned: covers only the three message-based charts
      (`ComfortCurveChart`, `TariffsLineChart`, `TimelineSeriesChart`), sharing layout/
      styling only, NOT message text (each keeps its own contextual wording) and NOT the
      2-point-placeholder pattern used by `AssetTimelineChart`/`TariffChart`/
      `StackedAreaChart` — that pattern exists to keep axes/NOW-line machinery rendering
      even with no data, a materially different need from a bare "no data" message, so
      forcing both into one strategy would have been a real behavior regression (losing
      axis rendering), not deduplication. All existing test IDs and exact message text
      (`getByText("No data for selected series")`) preserved unchanged.
- [x] 5.5 Unit tests: covered via the existing (unchanged, still-passing) test suites for
      each consumer — `AssetTimelineChart`/`TariffChart`/`StackedAreaChart` (NOW line +
      zone shading rendering, via their existing `xAxisTicks`/`referenceAreas`-capturing
      mocks), `ComfortCurveCard`/`RawDiagnostics` (empty-state rendering). No behavior
      changed for any of these five charts, so no new assertions were needed beyond
      confirming the existing 132 tests across these files still pass unchanged.

## 6. Composition: TimeSeriesChart

- [x] 6.1 Build `charts/TimeSeriesChart.tsx` — multi-axis (left + up to 2 right + 1 hidden),
      time X-axis, configured via a declarative series list (`{ key, axis, kind, color,
      unit, formatter }`), built on tasks 1–5's primitives. `tMin`/`tMax`/`nowMs`/
      `referenceAxisId` are all optional — not every consumer has a live "now" concept
      (discovered migrating `TariffsLineChart`, a planned-rates viewer with no NOW line).
- [x] 6.2 Migrate `AssetTimelineChart` to a `TimeSeriesChart` configuration; verified output
      is visually unchanged — all 56 existing tests across `AssetTimelineChart`/
      `AssetCell`/`Controller`/`History` pass with zero test-file edits needed. PV
      curtailment shading passed through via the new `extraReferenceAreas` prop, kept as
      this chart's own asset-specific logic (classification/zone-building), not shared.
- [x] 6.3 Migrate `TariffChart` to a `TimeSeriesChart` configuration with its third axis
      (task 9's axis-split, done earlier, carried through unchanged in this migration)
- [x] 6.4 Migrate `TariffsLineChart`, `TimelineSeriesChart` (raw-diagnostics) to
      `TimeSeriesChart` configurations using `DIAGNOSTIC_CHART_HEIGHT`. `TariffsLineChart`
      gains not just an axis floor but a full axis split (tariff €/kWh vs. CO2 intensity
      g/kWh, via a new `tightSpanDomain`+`MIN_CO2_INTENSITY_SPAN_G_KWH`) — migrating it
      surfaced the exact same squeeze bug just fixed in `TariffChart`, so fixed
      consistently rather than only adding a floor as originally scoped.
      **`SimProfileChart` excluded** — scope correction found during implementation: its
      X-axis is categorical (asset id via `dataKey="name"`), not temporal, so it doesn't
      fit `TimeSeriesChart` (or `CurveChart`, which is for a continuous numeric X). Left
      on its existing shared primitives (axisDomain/unitFormat/tooltipStyle/chartLayout)
      as its own minimal ~40-line component — a 4th genuinely distinct shape, not
      duplication with anything else in the codebase.
- [x] 6.5 Deleted the old bespoke implementations by rewriting them in place (not a
      separate delete step) — each file's content became its `TimeSeriesChart`
      configuration directly, so there was no parallel old/new pair to later remove.
      `SimProfileChart.tsx` intentionally not touched (see 6.4).
- [x] 6.6 Unit tests: `AssetTimelineChart` needed zero test changes; `TariffChart.test.tsx`
      updated 3 assertions from string-`dataKey` to `name`-based lookup (the composition
      uses accessor-function dataKeys per the cursor-correctness contract, so raw dataKey
      is no longer a plain string to compare against — `name` is the same stable,
      human-readable identifier already used by the tooltip/legend). New coverage for the
      3rd tariff axis (task 9) and `TariffsLineChart`'s axis split. 487/488 tests pass
      throughout (same pre-existing, unrelated network failure); typecheck and lint clean
      at every step.

## 7. Composition: StackedTimeSeriesChart

> Scope note: by the time this group started, `StackedAreaChart` already shared 100% of
> its logic with the other compositions via the Groups 1–5 primitives — this group ended
> up being a formal rename/relocation for taxonomy consistency, not further deduplication.
> Confirmed with the user before proceeding rather than assumed.

- [x] 7.1 Build `charts/StackedTimeSeriesChart.tsx` on tasks 1–5's primitives, keeping
      `StackedAreaChart`'s existing pos/neg stacking and `StackedAreaTooltip`
      net-value-aggregation logic as this composition's own code (not shared, per
      design.md Decision 1). `StackedAreaTooltip`'s own name unchanged — it's an
      implementation-detail helper describing its behavior, not the public composition.
- [x] 7.2 Migrate `StackedAreaChart` call sites to `StackedTimeSeriesChart` — 3 sites
      (`GridAccumulatedCell.tsx`, `PlanPowerStack.tsx`, and the two test files that
      referenced it by path/export name); verified visually unchanged (pure rename, no
      logic touched)
- [x] 7.3 Deleted the old `StackedAreaChart.tsx` via `git mv`/rewrite-in-place, not a
      separate delete step
- [x] 7.4 Unit tests: existing `StackedAreaChart` test suite (renamed to
      `StackedTimeSeriesChart.test.tsx`, same 68 assertions) passes against the new
      composition unchanged. Full repo sweep confirmed zero remaining references to the
      old name/path.

## 8. Composition: CurveChart

- [x] 8.1 Build `charts/CurveChart.tsx` sharing only sizing, empty-state, and
      unit-formatting primitives (no time-domain/NOW-line/zone-shading — none apply to a
      non-temporal X-axis)
- [x] 8.2 Migrate `ComfortCurveChart` to `CurveChart`; empty-state copy reused verbatim
      ("Add points to preview the curve") — design.md's open question resolved with no
      product/UX sign-off needed since the message itself didn't change, only its
      component name/location
- [x] 8.3 Deleted the old `ComfortCurveChart.tsx` via rename, not a separate delete step
- [x] 8.4 Unit tests: existing `ComfortCurveCard`/`Devices` test suites (38 tests) pass
      against the new composition unchanged. Kept scoped to its one real (fill%, €/kWh)
      shape rather than generalized further — no second non-temporal-X chart exists yet
      to generalize for.

## 9. TariffChart's third axis

- [x] 9.1 Split the current single `yAxisId="tariff"` axis: left axis carries
      `importPriceEurKwh`/`exportPriceEurKwh` only, unit label `€/kWh`, own
      `minSpanDomain` floor (task 2.2); new right axis `cost` carries
      `totalCostRateEurH`, unit label `€/h`, own floor; existing right axis `co2`
      unchanged. Done directly in the current `TariffChart.tsx` (ahead of the task 6.3
      composition migration) since the fix doesn't depend on the new composition existing.
- [x] 9.2 Keep the NOW `<ReferenceLine>` anchored to the (now tariff-only) left axis
- [x] 9.3 Unit tests: tariff series' rendered domain is independent of cost-rate's range
      (construct a fixture where cost rate's magnitude would previously have flattened
      tariff, assert tariff's rendered range is now unaffected); axis unit labels read
      `€/kWh` and `€/h`, never bare `€`

## 10. Migrate call sites

> Done incrementally as each migration/rename landed (Groups 6–8), rather than as a
> separate batch — `AssetTimelineChart.tsx`/`TariffChart.tsx` were rewritten in place
> (same file path, same export name), so `History.tsx`/`AssetMidSection.tsx`/
> `GridTariffCell.tsx` never needed an import change at all. `GridAccumulatedCell`/
> `PlanPowerStack` (task 7) and the Devices comfort-curve editor (task 8) were updated as
> part of their respective rename commits. This group is now just the confirmation sweep.

- [x] 10.1 `History.tsx` — no change needed; `AssetTimelineChart`/`TariffChart` kept their
      file path and export name through migration
- [x] 10.2 `AssetMidSection.tsx` (Controller tab) — same, no change needed
- [x] 10.3 `GridAccumulatedCell` — updated to `StackedTimeSeriesChart` in task 7's commit
- [x] 10.4 `GridTariffCell` — no change needed (imports `TariffChart` by its unchanged
      path/name; only its `chartLayout`/`axisDomain` imports moved, updated in Group 1/2)
- [x] 10.5 Devices tab comfort-curve editor (`ComfortCurveCard.tsx`) — updated to
      `CurveChart` in task 8's commit
- [x] 10.6 Raw Diagnostics page — no change needed; `SimProfileChart`/`TariffsLineChart`/
      `TimelineSeriesChart` kept their file paths and export names through migration
- [x] 10.7 Full grep sweep confirms zero remaining references to any deleted path
      (`controller/chartLayout`, `controller/charts/axisDomain`, `StackedAreaChart`,
      `ComfortCurveChart`, `CHART_COLORS`/`raw-diagnostics/colors`) anywhere in
      `VEN/ui/src`

## 11. Documentation & backlog

- [ ] 11.1 Update any architecture doc referencing the old chart component names/locations
      (check `docs/architecture/VEN_ARCHITECTURE.md` and similar for stale references)
- [ ] 11.2 Append `docs/history/project_journal.md` entry (what changed, why, the
      cursor-correctness invariant made structural) and `docs/reference/KEY_LEARNINGS.md`
      (index-based tooltip resolution as a recurring bug class; kit-of-primitives vs.
      universal-control trade-off)

## 12. Verification

- [ ] 12.1 `cd VEN/ui && npm test` — full suite green, including new kit/composition tests
- [ ] 12.2 `cd VEN/ui && npm run build`
- [ ] 12.3 ESLint zero errors
- [ ] 12.4 Manual visual check in a running dev server: Controller tab, History tab (both
      the "Last 24h" and date-picker views), Devices comfort-curve editor, Raw Diagnostics
      page — confirm each chart matches the visual-delta list in proposal.md's Impact
      section exactly (no chart should differ from that list, in either direction)
- [ ] 12.5 `scripts/audit_file_sizes.py` (new `charts/` files stay within VEN/ui size
      norms; no file balloons from absorbing multiple charts' logic)
