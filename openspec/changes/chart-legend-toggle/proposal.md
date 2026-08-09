## Why

Every Controller/History chart currently shows every series always-on, with no way to
isolate one line/area for inspection — on `AssetTimelineChart`'s 5+ overlapping series
(power, near/far forecast, cost, CO2, state) this makes it hard to focus on one signal.
Recharts' default `<Legend>` is purely decorative; it doesn't toggle series visibility.

Separately, `StackedTimeSeriesChart`'s legend shows **two** entries per asset (`"EV
(planned) +"` and `"EV (planned) -"`) for what is, to the user, one asset — the positive/
negative stack split is an internal rendering detail, not something the legend should
expose. This was found while scoping the toggle feature: consolidating pos/neg into one
legend entry per asset is a prerequisite for that chart's checkbox to mean anything ("hide
this asset," not "hide half of this asset's stack").

## What Changes

- `TimeSeriesChart` gains an opt-in `interactiveLegend?: boolean` prop. When set, the
  legend renders a small checkbox next to each series' label; clicking either toggles
  that series' visibility. All series start visible (unchanged default). State is local
  to the chart instance (resets to all-visible on remount, not persisted).
- `StackedTimeSeriesChart` gains the same opt-in capability, with each asset's positive
  and negative `<Area>` pair toggled together as one legend entry (fixing the double-entry
  legend as part of the same change, since the toggle needs asset-level granularity
  anyway) — the grid net-power line is a separate, independently-toggleable entry.
- Enabled on: `AssetTimelineChart` (Controller cells + History), `TariffChart` (Grid
  Tariff cell + History), `StackedTimeSeriesChart`'s Controller usage
  (`GridAccumulatedCell`). Not enabled on the raw-diagnostics `TimeSeriesChart` consumers,
  `CurveChart` (single series, toggling it would just empty the chart), or
  `StackedTimeSeriesChart`'s Planner usage (`PlanPowerStack`) — out of the requested scope
  (Controller/History diagrams).
- If `StackedTimeSeriesChart`'s asset-pair toggle proves impractical, it's acceptable to
  ship the feature on `AssetTimelineChart`/`TariffChart` only and leave
  `StackedTimeSeriesChart` on its current legend (still gaining the one-entry-per-asset
  fix on its own merits, decoupled from the toggle).

## Capabilities

### New Capabilities
- `chart-legend-toggle`: Controller and History charts let the user click a checkbox next
  to any series' legend label to show/hide that series live, without affecting any other
  chart instance or persisting across reloads.

### Modified Capabilities
- `StackedTimeSeriesChart`'s legend: one entry per asset instead of two (pos/neg no longer
  separately listed) — a correctness fix bundled with the toggle work since both require
  the same asset-level grouping.

## Impact

- **VEN UI**: `components/charts/TimeSeriesChart.tsx`, `components/charts/
  StackedTimeSeriesChart.tsx` (new legend-toggle logic); `components/controller/charts/
  AssetTimelineChart.tsx`, `components/controller/charts/TariffChart.tsx`,
  `components/controller/GridAccumulatedCell.tsx` (opt in via the new prop).
- **Non-goals**: no persistence of toggle state across reloads or between chart instances;
  no toggle on `CurveChart` or any raw-diagnostics/Planner chart; no change to what data
  any chart fetches or computes — purely a rendering/interaction change.
- No VTN, BFF, or backend Rust changes.
