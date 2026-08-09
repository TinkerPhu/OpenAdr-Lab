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

## Open Questions

None blocking.
