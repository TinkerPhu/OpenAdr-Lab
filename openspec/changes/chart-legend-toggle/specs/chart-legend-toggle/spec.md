## ADDED Requirements

### Requirement: A chart with an interactive legend lets the user toggle a series' visibility by clicking a checkbox next to its label
`TimeSeriesChart` and `StackedTimeSeriesChart` SHALL accept an opt-in
`interactiveLegend` flag. When set, each legend entry SHALL render a checkbox next to its
label; clicking the checkbox or the label SHALL toggle that series' visibility in the
chart. When unset, the chart's legend SHALL behave exactly as it did before this
capability existed.

#### Scenario: Unchecking a series hides it from the chart
- **WHEN** a chart has `interactiveLegend` enabled and the user unchecks a series' legend
  checkbox
- **THEN** that series is no longer rendered on the chart

#### Scenario: Re-checking a hidden series shows it again
- **WHEN** a previously-unchecked series' checkbox is checked again
- **THEN** that series renders on the chart again, in its original style

#### Scenario: Every series starts visible
- **WHEN** a chart with `interactiveLegend` enabled first renders
- **THEN** every series is visible and every legend checkbox is checked

#### Scenario: A chart without interactiveLegend is unaffected
- **WHEN** a chart does not set `interactiveLegend`
- **THEN** its legend renders with no checkboxes and no toggle behavior, identical to
  before this capability existed

### Requirement: Toggle state is local to each chart instance and does not persist
Each chart's set of hidden series SHALL be held in that chart instance's own local state.
No chart's toggle state SHALL be shared with, or affected by, another chart instance's
toggle state, and no chart's toggle state SHALL survive that chart instance unmounting.

#### Scenario: Two instances of the same chart toggle independently
- **WHEN** two separate instances of a chart with `interactiveLegend` enabled are
  rendered simultaneously, and a series is unchecked in one instance
- **THEN** the same series remains checked and visible in the other instance

#### Scenario: Toggle state resets on remount
- **WHEN** a chart instance with a hidden series unmounts and a new instance of the same
  chart mounts in its place
- **THEN** the new instance renders with every series visible, regardless of the previous
  instance's toggle state

### Requirement: A hidden series does not appear in the tooltip
When a series is toggled hidden, it SHALL NOT appear in the chart's hover tooltip.

#### Scenario: Hovering a chart with a hidden series omits that series' tooltip row
- **WHEN** a series is toggled hidden and the user hovers the chart
- **THEN** the tooltip does not show a row for that series

### Requirement: StackedTimeSeriesChart shows exactly one legend entry per asset, regardless of whether the toggle is enabled
`StackedTimeSeriesChart`'s legend SHALL show one entry per asset (not one entry per
internal positive/negative series), and one entry for the grid net-power line — this
grouping SHALL apply whether or not `interactiveLegend` is set.

#### Scenario: An asset with both positive and negative values shows one legend entry
- **WHEN** an asset has non-zero values in both its positive and negative series
- **THEN** the legend shows exactly one entry for that asset, not two

#### Scenario: Toggling an asset hides both its positive and negative series together
- **WHEN** `interactiveLegend` is enabled and the user unchecks an asset's legend entry
- **THEN** both that asset's positive and negative series are hidden from the chart

#### Scenario: The grid line has its own independent legend entry
- **WHEN** the chart renders the grid net-power line alongside per-asset stacked series
- **THEN** the grid line has its own legend entry, toggleable independently of any asset

### Requirement: A TimeSeriesChart series with no data anywhere in the current window is not rendered and has no legend entry
`TimeSeriesChart` SHALL exclude, from both its rendered series and its legend, any
declared series whose value is absent (null/undefined) at every row of the current `data`.
This exclusion SHALL apply whether or not `interactiveLegend` is set, and SHALL require no
per-series presence check at the call site — a chart declares every series it conceptually
has; presence is derived from the data, not declared by the caller.

#### Scenario: A series with no data anywhere in the window has no legend entry
- **WHEN** a chart declares a series whose value is absent at every row of `data`
- **THEN** that series has no entry in the legend, interactive or not

#### Scenario: A series that gains data on a later render gets a legend entry
- **WHEN** a chart re-renders with new `data` in which a previously-all-absent series now
  has a value at one or more rows
- **THEN** that series appears in the legend on that render, with no code change or
  caller-side flag required

#### Scenario: A series with only zero values (not absent) still gets a legend entry
- **WHEN** a series has a real value of exactly 0 at every row (present, not absent)
- **THEN** that series still appears in the legend — zero is data, not absence

### Requirement: Each TimeSeriesChart series declares its own tooltip formatter; the composition does not branch on the hovered series' name
`TimeSeriesSeriesSpec` SHALL support an optional per-series `formatter`. When a series
declares one, `TimeSeriesChart`'s tooltip SHALL use it for that series' value, looked up by
the series' own identity — not by a chart-level function branching on the hovered series'
display name.

#### Scenario: A series' own formatter is used for its tooltip value
- **WHEN** a series declares a `formatter` and the user hovers a point on that series
- **THEN** the tooltip value for that series is produced by that series' own `formatter`

#### Scenario: A series without its own formatter falls back to the chart-level default
- **WHEN** a series does not declare a `formatter` and the chart provides a fallback
  `tooltipFormatter`
- **THEN** the tooltip value for that series is produced by the fallback

### Requirement: StackedTimeSeriesChart's per-asset Area rendering and legend entries are derived from one shared list
The positive series, the negative series, and the legend entry for a given asset SHALL be
derived from one shared per-asset data structure, not independently re-computed in more
than one place.

#### Scenario: An asset's label and color are consistent between its Areas and its legend entry
- **WHEN** an asset's positive/negative `<Area>` elements and its legend entry are
  rendered
- **THEN** they use the identical label and color, sourced from the same per-asset record

### Requirement: ChartLegend renders a checkbox and a label, without a separate color swatch
Each `ChartLegend` entry SHALL render, at most, a checkbox (when interactive) and a
label — no separate colored swatch element in addition to the checkbox.

#### Scenario: An interactive legend entry has no extra color swatch
- **WHEN** an interactive legend entry is rendered
- **THEN** it contains a checkbox and a label, and no additional colored square element
