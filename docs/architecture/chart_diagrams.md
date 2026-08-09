# Chart/Diagram Architecture (VEN UI)

Every chart in `VEN/ui` (Controller tab, History tab, Devices comfort-curve editor, Raw
Diagnostics page) is built from one shared kit of primitives plus three named
compositions, all under `VEN/ui/src/components/charts/`. VTN UI has no charts.

## Directory layout

```
VEN/ui/src/components/charts/
  chartLayout.ts          sizing constants
  axisDomain.ts            axis-domain flooring, zero-anchored ticks, X-axis tick generation
  unitFormat.ts             per-physical-unit tooltip/tick formatting
  mergeSeries.ts            the cursor-correctness data model
  NowLine.tsx                the "NOW" reference line
  ZoneShading.tsx            zone background shading
  tooltipStyle.ts            shared tooltip container styling
  EmptyState.tsx             shared "no data" message treatment
  useLegendToggle.ts          local per-series show/hide state for interactive legends
  ChartLegend.tsx             shared checkbox-per-entry legend row
  TimeSeriesChart.tsx        composition 1: multi-axis line/step, time X-axis
  StackedTimeSeriesChart.tsx composition 2: stacked areas + net-value tooltip
  CurveChart.tsx             composition 3: non-temporal X-axis
  testUtils/
    assertTooltipMatchesData.ts  regression helper for the cursor-correctness invariant
```

Series/asset colors live in `VEN/ui/src/components/controller/types.ts`
(`ASSET_COLORS`, `SERIES_COLORS`) rather than under `charts/`, since they're consumed
by non-chart controller UI too (asset labels, legends).

## Why: the cursor-correctness invariant

Recharts resolves a hovered tooltip's value **by array index** into whatever `data`
array each `<Line>`/`<Area>` was given — not by matching timestamps across series. If a
chart renders two series from two independently-indexed arrays (e.g. a 1-minute actual
line and a 5-minute forecast line), hovering point `i` can show the actual line's real
value next to a forecast value from `otherArray[i]` — an unrelated timestamp.

**The rule**: every chart that renders more than one series builds ONE
timestamp-keyed row array before rendering (`mergeSeries.ts`'s
`mergeTimestampedSeries`/`locfFillKeys`), and every `<Line>`/`<Area>` reads its value via
a `dataKey` accessor into that single array — never its own independent `data` prop. The
`TimeSeriesChart`/`StackedTimeSeriesChart` compositions structurally enforce this: there
is no prop path in either component that accepts a per-series `data` override.

`testUtils/assertTooltipMatchesData.ts` is a reusable test helper: given a hovered
timestamp and the same accessor functions a chart passes as `dataKey`, it asserts each
accessor's value equals the merged row's own value for that series. Any chart's test
suite can call it as a regression guard.

**Adding a new multi-series chart**: build its data via `mergeTimestampedSeries` (fold
every series' samples into one array by timestamp) and, if any series is sampled sparser
than the array's own grid, `locfFillKeys` (forward-fills a step-function line so hovering
the plateau between two real samples shows a value, not a gap). Never give an individual
`<Line>` its own `data` array.

## Shared kit primitives

### `axisDomain.ts`

- `minSpanDomain(values, minSpan)` — floors a Y-axis domain to at least `minSpan`,
  anchored at 0. Correct for **rates with a meaningful zero baseline** (cost rate,
  CO2 rate, power) — "no cost"/"no CO2" is itself informative, so the axis always
  includes it.
- `tightSpanDomain(values, minSpan)` — floors a Y-axis domain to at least `minSpan`,
  fit tightly to the real data (like recharts' own `["auto","auto"]`), expanding only
  symmetrically around the data's own center when necessary. **Never anchors at 0.**
  Correct for **strictly-positive quantities without a meaningful zero baseline**
  (tariff €/kWh, CO2 intensity g/kWh) — `minSpanDomain` would seed the domain at 0 and
  compress a narrow real range (e.g. 0.28–0.32) into a sliver of the axis, exactly the
  "squeezed curve" defect a floor exists to prevent.
- `zeroAnchoredTicks(domain, targetTickCount)` — when a resolved domain has
  `min < 0 < max`, returns an explicit tick set guaranteed to include `0.0`, stepped
  outward from it in both directions using a "nice" (1/2/5×10ⁿ) step size. Returns
  `undefined` (defer to recharts' own tick generation) when the domain doesn't straddle
  zero. Pass the result as a `<YAxis ticks={...}>` prop.
- `formatPowerTick(valueKw)` — magnitude-aware power formatting: Watts (integer) below
  1 kW, kW (2 decimals) at/above. Used as the power axis's `tickFormatter` and, via
  `unitFormat.ts`, the power tooltip formatter — so axis and tooltip can never disagree.
- `roundedTimeTicks(fromMs, toMs, intervalMinutes)` — X-axis ticks snapped to the
  wall-clock (e.g. 10:00, 10:30) instead of recharts' domain-relative "nice" ticks;
  falls back to hourly spacing when the requested interval would produce more than 16
  labels.
- Floor constants: `MIN_POWER_SPAN_KW`, `MIN_COST_RATE_SPAN_EUR_H`,
  `MIN_CO2_RATE_SPAN_G_H`, `MIN_TARIFF_SPAN_EUR_KWH`, `MIN_CO2_INTENSITY_SPAN_G_KWH`.

### `unitFormat.ts`

One formatting rule per physical unit, used by every chart's tooltip (and, for power,
the axis tick):

| Unit | Rule |
|---|---|
| Power (kW) | Magnitude-aware: Watts (integer) below 1 kW, kW (2dp) at/above (`formatPowerValue`) |
| Signed power (kW) | Same, with explicit `+`/`-` prefix decided from the real value, not the rounded string (`formatSignedPowerValue`) — needed because a sub-watt negative residual (e.g. -0.0002 kW) rounds to `Math.round(-0.2)` = `-0`, which stringifies as `"0"`, silently losing the sign if decided from the formatted text |
| Cost rate (€/h) | 4 decimal places (`formatCostRateEurH`) |
| CO2 rate (g/h) | 1 decimal place (`formatCo2RateGH`) |
| CO2 intensity (g/kWh) | 3 decimal places (`formatCo2IntensityGKwh`) — a distinct physical quantity from CO2 rate, not the same rule |
| Tariff (€/kWh) | 4 decimal places (`formatTariffEurKwh`) |
| State of charge (%) | Input is a 0–1 fraction; 1 decimal place (`formatSocPct`) |
| Temperature (°C) | 1 decimal place (`formatTemperatureC`) |

### `NowLine.tsx` / `ZoneShading.tsx`

`renderNowLine(yAxisId, nowMs)` and `renderZoneShading(yAxisId, zones)` are **functions
that return elements directly**, not wrapping components — call them inline as
`{renderNowLine(...)}`/`{renderZoneShading(...)}` inside a `<ComposedChart>`. Recharts
inspects its direct children's types to compute axis domains and positioning for
`ReferenceLine`/`ReferenceArea`; wrapping either in an intermediate custom component
would change what type recharts sees at that position in the tree. A function call
spliced inline produces the identical `<ReferenceLine>`/`<ReferenceArea>` element(s), so
this avoids that risk entirely.

### `tooltipStyle.ts`

`TOOLTIP_CONTENT_STYLE`/`TOOLTIP_ITEM_STYLE`/`TOOLTIP_LABEL_STYLE` for the declarative
`<Tooltip contentStyle={...} />` prop path. `TOOLTIP_BOX_STYLE` replicates recharts' own
default tooltip box chrome (background/border/radius) for custom, non-declarative
tooltip content components that must aggregate multiple series before rendering (e.g.
`StackedTimeSeriesChart`'s net-value tooltip) and so can't use `contentStyle`.

### `EmptyState.tsx`

Shared layout (centered message, configurable height) for charts whose empty state is a
message rather than a still-rendered axis — `CurveChart`, and the raw-diagnostics
`TimeSeriesChart` consumers (`TariffsLineChart`, `TimelineSeriesChart`). Message text is
NOT shared — each caller keeps its own contextual wording (e.g. "Add points to preview
the curve" vs. "No tariff data"), since the text itself carries real information.

This is a deliberately different mechanism from the 2-point-placeholder pattern
`TimeSeriesChart` configurations for `AssetTimelineChart`/`TariffChart` use internally
when their `data` prop is empty (a 2-point array spanning `[tMin, tMax]`) — that pattern
exists so axes and the NOW line still render with no data, which a bare message can't do.

### `useLegendToggle.ts` / `ChartLegend.tsx`

Interactive per-series legend: `useLegendToggle()` is a local (unpersisted, not shared
across chart instances) `Set<string>` of hidden series keys, exposing `isHidden(key)`/
`toggle(key)`. `ChartLegend` renders one `[checkbox] [color swatch] label` row per entry
— the checkbox is only rendered (and clickable) when its `interactive` prop is true;
with `interactive=false` it renders the identical row layout with no checkbox, which is
what lets `StackedTimeSeriesChart` use one code path for both its always-on
one-entry-per-asset grouping and its opt-in toggle (see "Interactive legend" under
"The three compositions" below).

Every `<Line>`/`<Area>` a composition renders gets recharts' own `hide` prop wired to
`isHidden(key)` — this is the native recharts mechanism (not custom logic), and it
removes a hidden series from the tooltip's payload as well as from rendering.

### Sizing (`chartLayout.ts`)

`CELL_CHART_HEIGHT` (140px) is the height for Controller/History dashboard cells;
`DIAGNOSTIC_CHART_HEIGHT` (260px) is for the taller, full-page Raw Diagnostics charts —
two deliberately distinct constants, not one shared value, since the two contexts have
genuinely different layout needs.

### Colors

`ASSET_COLORS` (per-asset-id) and `SERIES_COLORS` (`import_tariff`, `export_tariff`,
`cost_rate`, `co2_rate`, `grid_line`, `power`) in `controller/types.ts` are the single
color source for every chart — no chart selects a color by positional array index.

## The three compositions

### `TimeSeriesChart`

Multi-axis (left + up to 2 right + 1 hidden), time X-axis. Configured declaratively via
`axes: TimeSeriesAxisSpec[]` and `series: TimeSeriesSeriesSpec[]`; every `dataKey` reads
from the one merged `data` array passed in (see the cursor-correctness invariant above).
`tMin`/`tMax`/`nowMs`/`referenceAxisId` are all optional — a chart with no live "now"
concept (e.g. a planned-rates viewer) omits them and gets recharts' own `["auto","auto"]`
X-domain and no NOW line. `interactiveLegend?: boolean` (default false) opts into the
checkbox-per-series legend (`ChartLegend`) described above; unset, the legend is the
plain recharts `<Legend>`, unchanged from before that capability existed.

Used by:
- **`AssetTimelineChart`** (Controller cells, History) — power/cost/CO2/hidden-state
  axes; forecast near/far overlay lines (conditionally rendered); PV curtailment shading
  passed via `extraReferenceAreas` (kept as `AssetTimelineChart`'s own asset-specific
  classification logic, not a shared chart concern — see "Special features" below).
  `interactiveLegend` enabled.
- **`TariffChart`** (Controller Grid Tariff cell, History) — tariff/cost/CO2 3-axis
  split (see "Special features"); its own domain-clipping/carry-forward logic for
  windowing `/tariffs` data to `[tMin, tMax]` is kept as this chart's own code, not
  shared, since it's specific to how that endpoint's data needs windowing.
  `interactiveLegend` enabled.
- **`TariffsLineChart`**, **`TimelineSeriesChart`** (Raw Diagnostics page) — simpler
  single/multi-series diagnostic views, no NOW line, `DIAGNOSTIC_CHART_HEIGHT`.
  `interactiveLegend` not enabled (out of the Controller/History scope this capability
  shipped for).

**Not used by `SimProfileChart`** (Raw Diagnostics) — its X-axis is categorical (asset
id, `dataKey="name"`), not temporal, so it doesn't fit this composition or `CurveChart`.
It stays a small (~40-line), standalone component built directly on `axisDomain.ts`/
`unitFormat.ts`.

### `StackedTimeSeriesChart`

Stacked positive/negative `<Area>` series (asset import above the X-axis, export/
generation below) plus a net grid-power `<Line>`, all sharing one power axis. Kept as
its own composition rather than a `TimeSeriesChart` configuration because the stacking
and the tooltip's pos/neg-to-net re-aggregation (`StackedAreaTooltip`, merging
`"${assetId} +"`/`"${assetId} -"` payload entries back into one net-kW row per asset)
are genuinely different logic from a plain multi-line chart — not duplication of it.
Built on the same shared axis/tick/color/sizing/NOW-line/zone-shading primitives.

Its legend always shows exactly **one entry per asset** (via `ChartLegend`), never one
per internal `${id}_pos`/`${id}_neg` series — this grouping applies unconditionally,
regardless of `interactiveLegend`, since the pos/neg split is an internal rendering
detail that should never have been user-visible in the first place. With
`interactiveLegend` enabled, unchecking an asset's entry hides both its positive and
negative `<Area>` together (the checkbox controls the asset as the user understands it,
not its two internal series); the grid net-power line has its own, independently
toggleable entry.

Used by the Controller Grid Accumulated cell (`GridAccumulatedCell.tsx`,
`interactiveLegend` enabled) and the Planner tab's plan-power preview
(`PlanPowerStack.tsx`, `interactiveLegend` not enabled — still gets the
one-entry-per-asset grouping, since that applies unconditionally).

### `CurveChart`

Non-temporal X-axis (currently: fill % vs. €/kWh bid price). Shares only sizing,
empty-state, and unit-formatting primitives — no time domain, NOW line, or zone shading
applies to a non-temporal axis. Has one real consumer (the Devices tab comfort-curve
editor preview, `ComfortCurveCard.tsx`); kept scoped to that exact shape rather than
generalized further, since there's no second non-temporal-X chart in the codebase yet to
generalize for.

## Special features

**TariffChart's 3-axis split** — import/export tariff (€/kWh, left axis,
`tightSpanDomain` floor), cost rate (€/h, own right axis, `minSpanDomain` floor), and
CO2 rate (g/h, own right axis, `minSpanDomain` floor) each get an independent Y-axis.
Tariff and cost rate are different physical dimensions (a price vs. a rate) and must
never share a scale — plotting them together previously let cost rate's larger range
visually flatten the tariff curves.

**Zero-anchored ticks** — any Y-axis whose domain straddles zero (mixed-sign data, e.g.
net grid power, or cost/CO2 rates that go negative during export) renders 0.0 as one of
its ticks, with the remaining ticks stepped outward from it symmetrically
(`zeroAnchoredTicks`). Wired into every `<YAxis>` across all three compositions whose
domain can plausibly be mixed-sign.

**PV curtailment shading** (`AssetTimelineChart`, `pvCurtailment` prop) — classifies
each point as hardware-capped (neutral shading), planned imposed curtailment (amber), or
unplanned imposed curtailment (red, past only), from `values.generation_limit_kw`/
`curtailment_source`/`inverter_max_kw` (past points) or `values.pv_forecast_kw` (future/
plan points). See `openspec` history for `pv-curtailment-history` for the underlying
data model; the classification and zone-building logic (`classifyPvPoint`,
`buildCurtailmentZones`) lives in `AssetTimelineChart.tsx` itself, passed to
`TimeSeriesChart` via the generic `extraReferenceAreas` prop rather than being a shared
chart-kit concern, since it's specific to this one asset's domain model.

**Forecast-accuracy overlay** (`AssetTimelineChart`, `nearForecast`/`farForecast` props,
History page only, for PV/base_load/site-residual) — near-lead and far-lead forecast
samples folded into the same merged data array as the actual Power line (never their own
override array — this is the cursor-correctness invariant applied to this specific
feature), forward-filled (LOCF) so their step-function lines have a value at every slot
between two real samples.
