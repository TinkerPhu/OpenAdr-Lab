# Chart/Diagram Architecture (VEN UI)

Every chart in `VEN/ui` (Controller tab, History tab, Devices comfort-curve editor, Raw
Diagnostics page) is built from one shared kit of primitives plus three named
compositions, all under `VEN/ui/src/components/charts/`. VTN UI has no charts.

## Directory layout

```
VEN/ui/src/components/charts/
  chartLayout.ts          sizing constants
  axisDomain.ts            axis-domain flooring, rounded Y ticks, X-axis tick generation
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
- `niceAxis(domain)` → `{ domain, ticks, step }` — the Y-axis counterpart of
  `roundedTimeTicks`: ticks land on exact multiples of a 1/2/5×10ⁿ step, and the domain is
  snapped outward to whole steps so the extreme labels are round too. It picks the
  *coarsest* step that still yields 3–7 ticks without inflating the domain past 1.5× the
  real data span — coarsest-wins keeps labels at one or two significant digits in the
  common case (`-0.6 / -0.4 / -0.2 / 0`), while a narrow band far from zero still gets the
  finer step it needs (`1.3 / 1.4 / 1.5`). 0 is always a tick whenever the domain contains
  it, which is what the removed `zeroAnchoredTicks` used to provide for mixed-sign domains
  only. **Callers never call this**: the compositions apply it to every axis themselves
  (see below).
- `tickFormatterForStep(step)` — label formatter matching a `niceAxis` step: exactly the
  decimals the step implies, so float noise never reaches a label. Used as the default
  axis `tickFormatter` when an axis declares no unit-specific one of its own.
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
`toggle(key)`. `ChartLegend` renders one `[checkbox] label` row per entry — the checkbox
is itself color-tinted via `accentColor`, and the label text is color-tinted too, so no
separate swatch element is needed. The checkbox is only rendered (and clickable) when its
`interactive` prop is true; with `interactive=false` it renders the identical row layout
with no checkbox, which is what lets `StackedTimeSeriesChart` use one code path for both
its always-on one-entry-per-asset grouping and its opt-in toggle (see "Interactive
legend" under "The three compositions" below).

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

**Data-presence filtering** — a caller declares every series it conceptually has, even
ones that may have no data in a given render (e.g. `AssetTimelineChart` always declares
Cost rate/CO₂eq rate, whether or not the underlying asset has cost/CO2 data attached).
`TimeSeriesChart` itself computes `visibleSeries = series.filter(s => seriesHasData(data,
s.dataKey))` (`mergeSeries.ts`'s `seriesHasData` — true if any row's accessor yields a
non-null value) and uses `visibleSeries` for both the rendered `<Line>`s and the legend
entries, regardless of `interactiveLegend`. This applies by construction to every current
and future series a caller declares — no caller writes its own `hasXData`-style boolean
per series (see `.claude/CLAUDE.md`'s `generic-over-bespoke` rule). A series whose real
value is exactly `0` at every row still counts as present (`0` is data, not absence).

**Per-series tooltip formatter** — `TimeSeriesSeriesSpec.formatter?: (value: number) =>
string` lets a series declare its own tooltip value formatting where it's declared,
instead of a chart-level `tooltipFormatter` branching on the hovered series' display name
(the `declare-dont-branch` rule in `.claude/CLAUDE.md`). Resolution: a hovered series'
own `formatter` if present, else the chart-level `tooltipFormatter` (now optional,
fallback-only), else `String(value)`.

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
detail that should never have been user-visible in the first place. An `assetSeries =
renderOrder.map(id => ({ id, label, color }))` array is built once and drives all three
derivations (the positive `<Area>` map, the negative `<Area>` map, and the legend
entries) — a shared derivation, not three independently-written ones, so an asset's
label/color can never drift between what's drawn and what the legend shows. The grid
net-power line stays a separate, hardcoded 4th legend entry, not part of the per-asset
family. Deliberately does NOT get `TimeSeriesChart`'s data-presence auto-filtering:
`StackedAreaPoint`'s pos/neg fields are always plain `number` (never `null`), so there's
no absence signal to filter on for this composition. With
`interactiveLegend` enabled, unchecking an asset's entry hides both its positive and
negative `<Area>` together (the checkbox controls the asset as the user understands it,
not its two internal series); the grid net-power line has its own, independently
toggleable entry.

Used by the Controller Grid Accumulated cell (`GridAccumulatedCell.tsx`,
`interactiveLegend` enabled) and the Planner tab's plan-power preview
(`PlanPowerStack.tsx`, `interactiveLegend` not enabled — still gets the
one-entry-per-asset grouping, since that applies unconditionally).

Both consumers build their `StackedAreaPoint[]` the same way, from the same source: the
shared `buildStackedFromAllTimelines()` (defined in `GridAccumulatedCell.tsx`, imported by
`PlanPowerStack.tsx`) zips per-asset series from `useAllTimelines()`
(`GET /timeline/all`). Its "grid" virtual asset carries `net_import_kw - net_export_kw`,
computed once server-side (`controller/timeline.rs`) — neither component re-derives grid
power from raw plan-slot fields itself. `PlanPowerStack` queries with `hoursBack: 0`
(forecast-only) where `GridAccumulatedCell` includes trailing history; that's the only
intentional difference between the two call sites. (Before this, `PlanPowerStack` built
its points independently from `usePlan()`'s raw `Plan` object and read only
`slot.net_import_kw`, silently dropping export — the grid line sat near zero under an
autarky objective even while the stack showed heavy PV export. Two independent
implementations of "plan slot → chart point" is what let one of them be wrong; there is
now exactly one.)

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

**Rounded Y ticks, enforced by the compositions** — a chart declares only its *data*
domain (`minSpanDomain`/`tightSpanDomain`); `TimeSeriesChart`, `StackedTimeSeriesChart` and
`CurveChart` each run it through `niceAxis` themselves before handing it to `<YAxis>`, so
every Y label is a round multiple of the axis step. `TimeSeriesAxisSpec` deliberately has
**no `ticks` prop** — there is no code path through which a caller can render un-rounded Y
labels, and no chart contains tick logic of its own. The two charts outside the kit that own
a bare `<YAxis>` (`raw-diagnostics/SimProfileChart`, `pages/PlanHistory`) call `niceAxis`
directly, for the same reason `NowLine`/`ZoneShading` are functions returning elements: a
wrapper component would change the child type recharts inspects.

This replaced an opt-in `zeroAnchoredTicks(domain)` helper that returned `undefined` for any
domain not straddling zero — so every single-sign axis (PV export revenue in €/h, CO2 in
g/h, a strictly-positive tariff in €/kWh) silently fell back to recharts ticking the raw
domain, producing labels like `-0.31275 €/h`, and each caller had to remember to opt in.

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
