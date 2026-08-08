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

- [ ] 5.1 Extract the NOW `<ReferenceLine>` block (color, dash pattern, label) duplicated
      in `AssetTimelineChart`/`StackedAreaChart`/`TariffChart` into `charts/NowLine.tsx`
- [ ] 5.2 Extract the `zones?.map(...)` `<ReferenceArea>` block into `charts/ZoneShading.tsx`
- [ ] 5.3 Extract tooltip container styling (`contentStyle`/`itemStyle`/`labelStyle`) into
      `charts/tooltipStyle.ts`, consumed by both the declarative `<Tooltip>` prop path and
      `StackedAreaTooltip`'s custom JSX
- [ ] 5.4 Extract one empty-state component (`charts/EmptyState.tsx`), replacing both the
      2-point-placeholder-array pattern and the `data.length === 0` early-return pattern
      with a single chosen strategy
- [ ] 5.5 Unit tests: NOW line renders at the correct x-position given `nowMs`; zone shading
      renders the given zones; empty-state component renders for zero-length data

## 6. Composition: TimeSeriesChart

- [ ] 6.1 Build `charts/TimeSeriesChart.tsx` — multi-axis (left + up to 2 right + 1 hidden),
      time X-axis, configured via a declarative series list (`{ key, axis, kind, color,
      unit, formatter }`), built on tasks 1–5's primitives
- [ ] 6.2 Migrate `AssetTimelineChart` to a `TimeSeriesChart` configuration; verify output
      is visually unchanged (per design.md — no intended visual delta for this chart)
- [ ] 6.3 Migrate `TariffChart` to a `TimeSeriesChart` configuration with its new third
      axis (task 9 covers the axis-split specifics)
- [ ] 6.4 Migrate `SimProfileChart`, `TariffsLineChart`, `TimelineSeriesChart`
      (raw-diagnostics) to `TimeSeriesChart` configurations using `DIAGNOSTIC_CHART_HEIGHT`;
      `TariffsLineChart` gains the axis floor it previously lacked (intentional behavior
      change per spec)
- [ ] 6.5 Delete the old `AssetTimelineChart.tsx`/`TariffChart.tsx`/`SimProfileChart.tsx`/
      `TariffsLineChart.tsx`/`TimelineSeriesChart.tsx` bespoke implementations once their
      call sites (task 10) point at the new configurations
- [ ] 6.6 Unit tests: each migrated chart's existing test suite passes against the new
      composition (update fixtures/selectors as needed, not test intent); new tests for
      the 3rd tariff axis and the raw-diagnostics color/precision/floor changes

## 7. Composition: StackedTimeSeriesChart

- [ ] 7.1 Build `charts/StackedTimeSeriesChart.tsx` on tasks 1–5's primitives, keeping
      `StackedAreaChart`'s existing pos/neg stacking and `StackedAreaTooltip`
      net-value-aggregation logic as this composition's own code (not shared, per
      design.md Decision 1)
- [ ] 7.2 Migrate `StackedAreaChart` call sites to `StackedTimeSeriesChart`; verify visually
      unchanged
- [ ] 7.3 Delete the old `StackedAreaChart.tsx`
- [ ] 7.4 Unit tests: existing `StackedAreaChart` test suite passes against the new
      composition

## 8. Composition: CurveChart

- [ ] 8.1 Build `charts/CurveChart.tsx` sharing only sizing, tooltip-style, and
      color-registry primitives (no time-domain/NOW-line/zone-shading)
- [ ] 8.2 Migrate `ComfortCurveChart` to `CurveChart`; resolve design.md's open question on
      empty-state copy (reuse existing message unless product/UX flags otherwise)
- [ ] 8.3 Delete the old `ComfortCurveChart.tsx`
- [ ] 8.4 Unit tests: existing `ComfortCurveChart` test suite passes against the new
      composition

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

- [ ] 10.1 `History.tsx` — update imports to the new `TimeSeriesChart`
      (`AssetTimelineChart`/`TariffChart`) configurations
- [ ] 10.2 `AssetMidSection.tsx` (Controller tab) — same
- [ ] 10.3 `GridAccumulatedCell` — update to `StackedTimeSeriesChart`
- [ ] 10.4 `GridTariffCell` — update to the migrated `TariffChart` configuration
- [ ] 10.5 Devices tab comfort-curve editor — update to `CurveChart`
- [ ] 10.6 Raw Diagnostics page — update to the migrated raw-diagnostics `TimeSeriesChart`
      configurations
- [ ] 10.7 Full grep sweep for any remaining import of a deleted component path; fix or
      confirm none remain

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
