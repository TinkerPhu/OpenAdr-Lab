## Why

`VEN/ui` has seven hand-built recharts trees (`AssetTimelineChart`, `StackedAreaChart`,
`TariffChart`, `ComfortCurveChart`, `SimProfileChart`, `TariffsLineChart`,
`TimelineSeriesChart`), each independently reimplementing axis-domain flooring, tick
formatting, tooltip styling/value formatting, the NOW reference line, zone shading,
sizing, and color selection. Git history shows the same bug classes fixed per-component,
repeatedly, instead of once centrally — most seriously, `117b44f` ("fix(ven): correct PV
forecast sign convention and history chart forecast overlay") fixed a cursor/tooltip
correctness bug in `AssetTimelineChart` where hovering a point could show a value from an
unrelated timestamp, because recharts resolves tooltip values by array index and the
actual-power line and forecast-overlay lines were fed from separately-indexed arrays. The
same root cause (misaligned per-series arrays) had already caused a near-identical bug in
`StackedAreaChart` months earlier (`f7b911e`), fixed independently and differently. Nothing
in the codebase today prevents a third chart from reintroducing this bug class — it was
fixed by one component happening to do it right, not by a shared, enforced pattern.

Two more concrete defects surfaced during review: `TariffChart`'s left Y-axis plots
`importPriceEurKwh`/`exportPriceEurKwh` (€/kWh) and `totalCostRateEurH` (€/h) — two
dimensionally different quantities — on one shared linear scale labeled generically `€`,
which visually flattens the tariff curves whenever the cost-rate series has a larger
range. And several Y-axes whose domain straddles zero (e.g. `StackedAreaChart`'s net grid
power) do not guarantee 0.0 is a rendered tick, so the zero-crossing point — the most
important reference point on those axes — is often invisible.

## What Changes

- Extract a shared chart kit (pure logic + small styled primitives, no chart-specific
  rendering) covering: axis-domain flooring and tick generation (extends the existing
  `axisDomain.ts`), zero-anchored tick generation when a domain straddles 0, per-unit
  tooltip/tick value formatting, tooltip container styling, the NOW reference line, zone
  shading, a single color registry, a sizing contract (two named height constants, not
  per-file literals), and one empty-state treatment.
- Introduce the canonical data-merge builder: every multi-series chart builds one
  timestamp-keyed row array (with LOCF fill for sparse series) before rendering, and every
  `<Line>`/`<Area>` reads its value via a `dataKey` accessor into that one array — never its
  own separately-indexed `data`. This is the structural fix for the `117b44f`/`f7b911e` bug
  class, generalized from where it already works in `AssetTimelineChart` to every chart
  that plots more than one series. A shared test helper asserts, for the composition layer,
  that a hovered tooltip's reported value equals the merged row's value for that series —
  turning the invariant into something a test catches, not something an author has to
  remember.
- Replace the seven ad hoc chart components with three compositions built on the shared
  kit — no fourth, "universal" chart abstraction, because `StackedAreaChart`'s pos/neg
  stacking and `ComfortCurveChart`'s non-temporal X-axis are different enough shapes that
  forcing one prop API to cover both would relocate duplication into branchy config instead
  of removing it:
  - `TimeSeriesChart` — multi-axis line/step chart, time X-axis. Covers
    `AssetTimelineChart`, `TariffChart`, `SimProfileChart`, `TariffsLineChart`,
    `TimelineSeriesChart`.
  - `StackedTimeSeriesChart` — built on the same kit, keeps its own stacking and
    pos/neg-to-net tooltip aggregation. Covers `StackedAreaChart`.
  - `CurveChart` — non-temporal X-axis, shares sizing/tooltip-style/color-registry
    primitives only. Covers `ComfortCurveChart`.
- `TariffChart` gains a third Y-axis: left = tariff (€/kWh, own `minSpanDomain` floor,
  corrected unit label), right = cost rate (€/h, own floor, new), right (unchanged) = CO₂
  rate (g/h). Import/export tariff no longer shares a scale with cost rate.
- Canonicalize decimal precision per unit across every chart/tooltip (see spec for the
  per-unit table); canonicalize the color registry (merge `CHART_COLORS`'
  positionally-indexed raw-diagnostics palette into the `ASSET_COLORS`-based registry,
  adding named semantic keys for tariff/cost/CO₂/grid so the same concept is always the
  same color across chart families).

## Capabilities

### New Capabilities
- `unified-chart-primitives`: a shared chart kit and three chart compositions
  (`TimeSeriesChart`, `StackedTimeSeriesChart`, `CurveChart`) that eliminate duplicated
  axis/tooltip/sizing/color logic across `VEN/ui`'s chart components, structurally prevent
  the cursor/tooltip index-mismatch bug class, guarantee a zero tick on any axis whose
  domain straddles zero, and fix `TariffChart`'s squeezed-axis and mislabeled-unit defects.

### Modified Capabilities
(none — this is UI-internal restructuring of existing chart behavior, not a new
user-facing capability; the "Modified" list below in Impact enumerates the concrete visual/
behavioral deltas per chart instead)

## Impact

- **VEN UI**: all seven existing chart components
  (`components/controller/charts/AssetTimelineChart.tsx`, `StackedAreaChart.tsx`,
  `TariffChart.tsx`, `components/devices/ComfortCurveChart.tsx`,
  `components/raw-diagnostics/SimProfileChart.tsx`, `TariffsLineChart.tsx`,
  `TimelineSeriesChart.tsx`), plus `components/controller/chartLayout.ts`,
  `components/controller/charts/axisDomain.ts`, `components/controller/types.ts`
  (`ASSET_COLORS` extended), `components/raw-diagnostics/colors.ts` (retired, folded into
  the extended registry). New shared kit module location TBD in planning (likely
  `components/charts/` at the top level, since it now serves both controller/History and
  raw-diagnostics call sites, not just controller/).
- **Visual/behavioral deltas** (see spec's per-chart requirements for exact scenarios):
  `AssetTimelineChart` and `StackedAreaChart` — no intended visual change, internals only.
  `TariffChart` — new third axis, corrected `€/kWh`/`€/h` unit labels, CO₂ tooltip decimal
  change, axis floor gained. `ComfortCurveChart` — tooltip styling and empty-state message
  change to match the shared primitive. Raw-diagnostics charts (`SimProfileChart`,
  `TariffsLineChart`, `TimelineSeriesChart`) — tariff/CO₂ series recolor to match the
  controller family's semantics, power tooltip switches to magnitude-aware W/kW format,
  `TariffsLineChart` gains an axis floor it previously lacked; height stays 260px via a
  named constant, not visually changed.
- No VTN, BFF, or backend Rust changes — this is entirely within `VEN/ui/src`.
- **Non-goals**: no single universal chart component (see Why); no change to what data any
  chart plots, only how axes/tooltips/sizing/colors are computed and rendered; no new
  chart types beyond the three compositions.
