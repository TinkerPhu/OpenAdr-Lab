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
