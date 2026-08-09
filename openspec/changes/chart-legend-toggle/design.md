## Context

Found while scoping: recharts' `<Legend>` is decorative only — it doesn't toggle series
visibility, and there's no built-in way to isolate one series on a busy multi-series chart
like `AssetTimelineChart` (power, near/far forecast, cost, CO2, state — 5+ series at once).
Separately, `StackedTimeSeriesChart` renders two `<Area>`s per asset (`${id}_pos`/
`${id}_neg`, for the above/below-x-axis stack split) and its legend shows both as separate
rows (`"EV (planned) +"`, `"EV (planned) -"`) — an internal rendering detail leaking into
the legend as apparent duplication.

## Goals / Non-Goals

**Goals:**
- Let a user click a checkbox next to any series' legend label to show/hide that series
  live, on `AssetTimelineChart`, `TariffChart`, and `StackedTimeSeriesChart`'s Controller
  usage (`GridAccumulatedCell`).
- Fix `StackedTimeSeriesChart`'s legend to show one entry per asset, unconditionally (not
  gated behind opting into the toggle) — a plain correctness fix, since the pos/neg split
  is not something the legend should ever have exposed.
- All series visible by default, unchanged from current behavior for every chart that
  doesn't opt in.

**Non-Goals:**
- No persistence of toggle state across reloads, tab switches, or between chart instances
  — each chart's toggle state is local, resets to all-visible on remount.
- No toggle on `CurveChart` (single series — toggling it would just empty the chart), any
  raw-diagnostics chart, or `StackedTimeSeriesChart`'s Planner usage (`PlanPowerStack`) —
  out of the requested Controller/History scope. `PlanPowerStack` still gets the
  one-entry-per-asset legend fix, since that's unconditional.
- No change to what data any chart fetches, computes, or how axes/domains are resolved —
  purely a legend-rendering and series-visibility interaction.

## Decisions

**1. One shared `ChartLegend` component, not two separate implementations.**
Both `TimeSeriesChart` and `StackedTimeSeriesChart` need the same visual — a row of
`[checkbox] [color swatch] label` entries — so it's a new shared primitive
(`charts/ChartLegend.tsx`), not duplicated per composition. Takes an `entries: {key,
label, color}[]` array, a hidden-keys set, a toggle callback, and an `interactive: boolean`
flag. When `interactive` is false, renders the same row layout without a checkbox
(indistinguishable from a normal legend) — this is what lets `StackedTimeSeriesChart` use
one code path for both its always-on grouping fix and its opt-in toggle.

**2. `TimeSeriesChart`'s default (non-`interactiveLegend`) path is untouched.**
Unlike `StackedTimeSeriesChart`, `TimeSeriesChart` consumers don't have a
grouping/duplication problem — each series already maps to exactly one legend entry today.
So when `interactiveLegend` is unset, `TimeSeriesChart` keeps rendering the plain
`<Legend iconSize={10} wrapperStyle={{fontSize:10}} />` exactly as before; `ChartLegend`
only replaces it when a consumer opts in. This keeps every non-opted-in
`TimeSeriesChart` consumer (the 3 raw-diagnostics charts) pixel-identical to before this
change.

**3. Toggling hides a series from the tooltip too, via recharts' own `hide` prop.**
Setting `hide={true}` on a `<Line>`/`<Area>` is recharts' native mechanism — it removes the
series from rendering AND from the tooltip's payload (a hidden series shouldn't appear
in the hover tooltip either, since there's nothing on the chart to point at). No custom
tooltip-filtering logic needed.

**4. Toggle state: a tiny local hook (`useLegendToggle`), not prop-driven.**
Each chart instance's toggle state (`Set<string>` of hidden series keys) is owned by a
`useState` inside a new `charts/useLegendToggle.ts` hook, instantiated once per composition
render. Not lifted to a parent/prop, since nothing outside the chart needs to read or set
it (matches the non-goal: no persistence, no cross-instance sharing).

**5. `StackedTimeSeriesChart`'s asset-level toggle hides both `_pos`/`_neg` Areas together.**
The `entries` passed to `ChartLegend` are per-asset (`assetIds.map(id => ({key: id, label:
assetLabel(id), color: colorMap[id]}))`), plus one more entry for the grid net-power line.
Toggling an asset's key sets `hide` on both its `${id}_pos` and `${id}_neg` `<Area>`s —
the checkbox controls the asset as the user understands it, not its two internal series.

**6. `AssetTimelineChart`'s hidden state-axis line is excluded from the legend/toggle.**
`stateKey`'s Line (SoC/T_tank) already renders on a fully hidden axis
(`axisLine={false}`/`tickLine={false}`/`tick={false}`) purely for tooltip-only values —
it's still a normal `series` entry so it still gets a legend/toggle entry like any other
series (no special-casing needed); toggling it off simply removes it from the tooltip too,
consistent with every other series.

## Risks / Trade-offs

- [`ChartLegend`'s checkbox row could wrap awkwardly on `AssetTimelineChart`'s narrow
  140px-tall Controller cells with 5+ series] → Bounded risk, same class of layout concern
  as the existing plain `<Legend>` already has at that width; `flexWrap: "wrap"` handles
  overflow the same way recharts' own Legend does. Needs a visual check post-implementation
  (same as the prior chart-primitives change), not assumed fine.
- [`StackedTimeSeriesChart`'s legend visual changes for its Planner usage
  (`PlanPowerStack`) too, even though the toggle itself isn't enabled there] → Intentional
  and disclosed (Goal: the one-entry-per-asset fix is unconditional) — the two-row-per-asset
  legend was a defect regardless of whether toggling is available.

## Correction pass (found during manual verification, 2026-08-09)

Deployed to Node1 for the manual check (task 6.4) and the user found two real issues plus
requested a cosmetic fix:

1. **Wrong control**: `AssetTimelineChart`'s legend showed checkboxes for "Cost rate" and
   "CO₂eq rate" even on cells where those series had no data at all (an all-null line,
   invisible on the chart). Root cause: unlike the near/far forecast lines
   (`hasNearForecast`/`hasFarForecast`-gated), Cost rate and CO₂eq rate were pushed into
   `series` unconditionally — the same latent gap exists in `TariffChart`'s 4 series.
2. **Cosmetic**: `ChartLegend` renders both a tinted checkbox AND a separate colored
   square swatch per entry — redundant, remove the swatch.

Scoping the fix surfaced a broader, explicitly-requested principle (now recorded as the
`generic-over-bespoke` and `declare-dont-branch` rules in `.claude/CLAUDE.md`): don't patch
this with a third one-off `hasXData` boolean: name the general pattern ("a declared series
may have no real data in the current window; only series with real data get a graph and a
legend entry") and solve it once, in the composition, so it applies by construction to
every current and future series — including deleting the two existing one-off booleans
(`hasNearForecast`/`hasFarForecast`) once the composition does this itself. The same
audit surfaced `tooltipFormatter`'s `if (name === "X") ... else if (name === "Y")` chains
in `AssetTimelineChart`/`TariffChart`/`TariffsLineChart` as the identical anti-pattern
applied to formatting instead of visibility — folded into the same correction pass.

### Additional Goals

- A `TimeSeriesChart` series with no non-null value anywhere in the current `data` window
  is automatically excluded from both rendering and the legend — no per-caller presence
  check required, for any current or future series.
- Every `TimeSeriesChart` series declares its own tooltip value formatter at the point the
  series itself is declared; `TimeSeriesChart` looks it up by series identity instead of
  any consumer branching on the hovered series' name string.
- `StackedTimeSeriesChart`'s per-asset `<Area>` pair and its legend entry are derived from
  one shared array, not two independently-written `.map()` calls over the same asset list
  (today they already share `renderOrder` as input, but as two positionally-parallel
  derivations kept in sync by convention, not by construction).
- Remove the redundant color swatch from `ChartLegend` — checkbox + label only.

### Additional Non-Goals

- **No data-presence filtering added to `StackedTimeSeriesChart`'s per-asset entries.**
  `StackedAreaPoint`'s `${id}_pos`/`${id}_neg` fields are always plain `number` (never
  `null`) — there is no signal in the type distinguishing "this asset was never sampled"
  from "this asset is genuinely at exactly 0 kW right now," unlike `TimestampedRow`'s
  `Record<string, number | null>` values map, which does carry that distinction. Treating
  "always zero across the window" as "absent" would hide a real, legitimately-idle asset
  (e.g. an EV with no active session) — a different, weaker, and untested claim than what
  was actually reported. Confirmed via `openspec/changes/unify-plan-power-stack-grid/`
  (an unrelated, unimplemented change touching `StackedTimeSeriesChart`'s data sourcing)
  that no other in-flight work depends on or conflicts with leaving this out of scope now.
- No change to which charts have `interactiveLegend` enabled (still just
  `AssetTimelineChart`/`TariffChart`/`GridAccumulatedCell`'s `StackedTimeSeriesChart` use)
  — this pass is a correctness/structure fix to what's already shipped, not a scope change.

### Additional Decisions

**7. Data-presence filtering lives in `TimeSeriesChart` itself, applied to `series` before
either rendering or building `ChartLegend`'s entries.**
A new `seriesHasData(data: TimestampedRow[], dataKey: TimeSeriesSeriesSpec["dataKey"]):
boolean` (in `mergeSeries.ts`, alongside the other data-model primitives) evaluates the
accessor (or string key) across every row; `TimeSeriesChart` computes
`visibleSeries = series.filter(s => seriesHasData(data, s.dataKey))` once and uses it for
both the `<Line>` map and the legend entries — since both already derive from one array
(Decision 2 above), this filter step automatically fixes the legend and the (already
harmless, since an all-null line draws nothing) rendering in one place. Applies whether or
not `interactiveLegend` is set — the plain, non-interactive legend had the same dead-entry
defect, just less visible without a checkbox drawing the eye to it.

**8. `hasNearForecast`/`hasFarForecast` are deleted from `AssetTimelineChart`, not
generalized into a third parallel check.**
Once `TimeSeriesChart` filters by data presence itself, `AssetTimelineChart` no longer
needs to compute whether the forecast props were non-empty before deciding whether to push
a series entry — it pushes the near/far forecast series unconditionally (same as Cost
rate/CO₂eq rate always were), and the composition hides them automatically when
`predicted_kw_near`/`predicted_kw_far` never got merged into any row. Net effect: less code
at the call site, not more, and no more asymmetry between the (gated) forecast lines and
the (ungated) cost/CO2 lines — one rule, not two.

**9. Per-series tooltip formatter, looked up by name, replacing the `tooltipFormatter`
if/else chain.**
`TimeSeriesSeriesSpec` gains an optional `formatter?: (value: number) => string`. The
composition's `tooltipFormatter` prop becomes optional, used only as a fallback when a
hovered series has no per-series `formatter` (kept for the simplest raw-diagnostics cases
that don't need per-series distinction). `AssetTimelineChart`/`TariffChart`/
`TariffsLineChart` each move their existing `unitFormat.ts` function references from an
`if (name === "...")` chain into each series' own `formatter` field — the formatter is now
declared once, next to the series it belongs to, instead of re-derived from a string
comparison against that series' display name every time the tooltip renders.
`TimelineSeriesChart` (already single-series) gets the same field for consistency, though
it had no branch to remove.

**10. `StackedTimeSeriesChart`'s Area+legend unification: one `assetSeries` array, two
consumers.**
`assetSeries = renderOrder.map(id => ({ id, label: assetLabel(id), color: colorMap[id] ??
COLOR_ASSET_FALLBACK }))`, computed once. The positive-`<Area>` map, the negative-`<Area>`
map, and `ChartLegend`'s `entries` all read from this one array instead of three
independent derivations from `renderOrder`/`colorMap`/`assetLabel`. Grid stays a separate,
hardcoded fourth entry (it isn't a member of the per-asset family — no unification benefit
from folding it in).

### Additional Risks / Trade-offs

- [Deleting `hasNearForecast`/`hasFarForecast` means the forecast `<Line>` elements are now
  always present in the `series` array, relying entirely on the new composition-level
  filter to hide them when absent] → Intentional; covered by task-level regression tests
  asserting the filtered-out state renders no `<Line>` and no legend entry when forecast
  props are empty, same assurance the old boolean gave, now provided structurally instead
  of by a caller-side check.
- [`seriesHasData` must handle both the `string` and accessor-function forms of `dataKey`]
  → Both already coexist in `TimeSeriesSeriesSpec` today; the helper accepts either, same
  as recharts' own `dataKey` prop does.

## Open Questions

None blocking.
