## Context

Found while investigating why the Controller/History diagrams keep needing axis-labeling,
sizing, and cursor-label fixes commit after commit:

1. Seven chart components (`VEN/ui/src`, all recharts-based; VTN UI has no charts at all)
   independently reimplement the same handful of concerns: time-domain fixing, tick
   formatting, axis-domain flooring, tooltip styling/value formatting, the NOW reference
   line, zone shading, sizing, color selection, empty-state handling.
2. The cursor-shows-wrong-number bug (`117b44f`) and its earlier twin (`f7b911e`,
   `StackedAreaChart`) share one root cause: recharts resolves a hovered tooltip's value
   *by array index*, so any chart that feeds two series from two separately-indexed arrays
   (e.g. a 1-minute actual-power series and a 5-minute forecast series) can silently show
   one series' value next to another series' timestamp. `AssetTimelineChart` fixed this by
   merging every series into one timestamp-keyed array and reading each `<Line>` via a
   `dataKey` accessor into that array — but this fix is local to one file; nothing enforces
   it anywhere else.
3. `TariffChart`'s left axis plots `€/kWh` (import/export tariff) and `€/h` (cost rate) —
   different physical dimensions — on one shared, unfloored, generically-`€`-labeled scale,
   flattening the tariff curves.
4. Axes whose domain straddles zero (mixed-sign, e.g. net grid power) don't guarantee 0.0
   is a rendered tick.
5. Two independent color sources exist: `ASSET_COLORS` (ID-keyed) and `CHART_COLORS`
   (a positional array, no IDs) — the same concepts (import tariff, export tariff, CO₂
   rate) render in different colors depending which chart family draws them.
6. Decimal precision per unit (power, cost, CO₂, tariff) differs by file with no
   shared rule — e.g. CO₂ rate is `.toFixed(1)` in `AssetTimelineChart` and `.toFixed(0)`
   in `TariffChart`; power precision is `.toFixed(3)`, `.toFixed(2)`, or a magnitude-aware
   W/kW switch depending which file you're in.

## Goals / Non-Goals

**Goals:**
- Eliminate duplicated axis/tick/tooltip/sizing/color logic across all seven chart
  components — zero tolerance for the same concern being reimplemented in more than one
  place.
- Make the cursor/tooltip-correctness invariant structural (impossible to violate by
  construction) rather than a convention one component happens to follow, and back it with
  a test that would have caught `117b44f`/`f7b911e`.
- Fix the three concrete defects found during review: `TariffChart`'s squeezed/mislabeled
  axis, missing zero-anchored ticks on mixed-sign domains, and the two-color-palette split.
- Preserve each chart's genuinely distinct visual behavior (stacking, non-temporal X-axis,
  raw-diagnostics' taller layout) as intentional, named exceptions built on the same kit —
  not forced into a single one-size-fits-all component.

**Non-Goals:**
- No single "universal" chart control. `StackedAreaChart` (pos/neg stacking + net-value
  tooltip re-aggregation) and `ComfortCurveChart` (non-temporal X-axis) are different enough
  shapes that a single component's prop surface would just relocate today's file-level
  duplication into branchy, hard-to-read configuration inside one file. Three compositions
  over one shared kit avoids both problems.
- No change to what data any chart plots or where that data comes from — this is entirely
  about how axes, tooltips, sizing, and colors are computed and rendered.
- No VTN UI chart work (VTN UI has no charts today; out of scope).
- No coverage-floor or new backend history/query changes.

## Decisions

**1. Kit of primitives + three named compositions, not one universal component.**
Considered forcing all seven charts through one configurable component. Rejected: the
prop surface needed to express stacking (`StackedAreaChart`) and a non-temporal X-axis
(`ComfortCurveChart`) alongside the five genuinely-uniform time-series charts would turn
into a branchy config object — the same duplication problem, just moved from separate
files into one file's conditional logic. Instead: one shared kit (domain/tick engine,
merge builder, tooltip primitive, NOW line, zone shading, sizing contract, color registry,
empty-state) with zero duplicated logic across consumers, and three thin compositions
(`TimeSeriesChart`, `StackedTimeSeriesChart`, `CurveChart`) that assemble kit primitives
for their specific shape.

**2. The cursor-correctness invariant is enforced by the merge builder's API shape, not
by convention.**
The compositions' `<Line>`/`<Area>` rendering only ever accepts a `dataKey` into the single
merged array the kit builder produces — there is no code path in any composition that lets
a caller pass an independent `data` array per series. This makes the `117b44f` bug class
unrepresentable, not just fixed-until-someone-forgets. A shared test helper in the kit
(reusable by every composition's test suite) asserts that for a given hovered timestamp,
each series' reported tooltip value equals `mergedRow[timestamp][series]` — regression-testing
the invariant directly rather than relying on visual review to catch a reintroduction.

**3. Canonical color registry: extend `ASSET_COLORS`, retire the positional `CHART_COLORS`.**
`ASSET_COLORS` is the more complete, ID-keyed, production-facing registry (asset IDs, NOW
line already named). `CHART_COLORS` has no ID keys at all — raw-diagnostics charts pick
colors by literal array index, which is how import tariff ended up blue in one chart family
and red (`COLOR_IMPORT_TARIFF`) in another. Resolution: add named semantic keys
(`import_tariff`, `export_tariff`, `cost_rate`, `co2_rate`, `grid_line`) to the registry
using the controller family's existing hex values (chosen as canonical since it's the more
elaborated, multi-consumer surface), and have raw-diagnostics charts consume those keys
instead of positional indices. This is an intentional, visible recolor on the Raw
Diagnostics page — flagged explicitly, not a silent side effect.

**4. Canonical per-unit decimal precision, resolved per the comparison table below —
already reflected in the spec's requirements.**
Power adopts the existing magnitude-aware axis-tick rule (`formatPowerTick`: Watts,
integer-rounded, below 1kW; kW to 2 decimals at/above) for tooltips too, replacing three
different fixed-precision rules. CO₂ rate settles on `.toFixed(1)` (chosen over
`TariffChart`'s `.toFixed(0)` because these rates are often small/fractional in this
system and integer rounding was hiding real digits). Cost (€/h), tariff (€/kWh), SoC (%),
temperature (°C) were already consistent wherever they appear and are simply codified as
the shared rule instead of re-derived per file. `TariffChart`'s catch-all tooltip branch
that labels every value `" €"` regardless of actual unit (€/kWh or €/h) is corrected as
part of switching to the shared unit-aware formatter.

**5. `TariffChart` gains a third axis: tariff (€/kWh) | cost (€/h) | CO₂ (g/h, unchanged).**
Applying the same "different physical dimension → different axis" rule
`AssetTimelineChart` already uses for power/cost/CO₂, to the one chart that never got it.
Layout mirrors `AssetTimelineChart`'s proven left + two-right-axes pattern in the same
140px cell height — not a new layout risk, a reuse of an already-working one. Each new
axis gets its own `minSpanDomain` floor; splitting the axis without flooring it would only
partially address the "squeezed to a flat curve" complaint.

**6. Raw-diagnostics height (260px) stays a deliberate, named exception, not unified to
140px.**
`SimProfileChart`/`TariffsLineChart`/`TimelineSeriesChart` are full-page diagnostic views,
each used exactly once, not dashboard cells — the size difference from the controller
family's `CELL_CHART_HEIGHT` (140px) is legitimate, not accidental drift. Resolution:
promote `260` to a second named constant (`DIAGNOSTIC_CHART_HEIGHT`) in the same shared
sizing contract, referenced three times instead of copy-pasted as a bare literal three
times. No visual change.

**7. Zero-anchored tick generation is a property of the shared domain/tick engine, not a
per-chart special case.**
When an axis's resolved domain `[min, max]` has `min < 0 < max`, the tick-generation
function builds the tick set by stepping outward from 0 in both directions instead of the
current start-anchored generation (which can skip 0 entirely). This is a change to
`axisDomain.ts`'s tick logic, inherited by every axis in every composition — not
reimplemented per chart.

## Risks / Trade-offs

- [Merging `ASSET_COLORS`/`CHART_COLORS` changes visible series colors on the Raw
  Diagnostics page] → Intentional and disclosed (see Decision 3); the alternative (keep
  two palettes) perpetuates the same-concept-different-color inconsistency this change
  exists partly to fix.
- [Canonicalizing power-tooltip precision to the magnitude-aware W/kW rule changes
  displayed tooltip text in `AssetTimelineChart`, `StackedAreaChart`, and two
  raw-diagnostics charts] → Intentional; matches the axis-tick rule those same charts
  already use, so tooltip and axis will finally agree with each other within a chart,
  which they currently don't.
- [`TariffChart`'s new third axis increases visible tick/label density in a 140px-tall
  cell] → Bounded risk: `AssetTimelineChart` already renders 3 visible + 1 hidden axis at
  the same height successfully. Needs a visual check post-implementation since
  `TariffChart`'s currency-formatted tick labels may be wider than `formatPowerTick`'s
  output, but this is a legibility check, not an open design question.
- [Retiring seven bespoke components in favor of three compositions is a real,
  multi-file refactor, not a small patch] → Scoped deliberately as its own change,
  separate from any feature work, per the project's `refactoring` rule (fix debt before
  adding new behavior in an affected area).

## Open Questions

- Exact target module path for the new shared kit (`components/charts/` at the top level
  vs. keeping it under `components/controller/` with raw-diagnostics importing across
  folders as it does today) — to be settled during planning, not blocking the spec.
- Whether `ComfortCurveChart`'s empty-state message text/style, once switched to the
  shared empty-state primitive, needs product/UX sign-off on new copy, or can reuse the
  existing message verbatim — to be resolved during implementation.
