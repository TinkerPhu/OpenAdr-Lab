## ADDED Requirements

### Requirement: A single shared chart kit provides axis, tooltip, sizing, and color logic with no duplication across chart components
`VEN/ui`'s chart components SHALL share one kit of primitives for: axis-domain flooring,
tick generation, per-unit value formatting, tooltip container styling, the NOW reference
line, zone shading, sizing constants, color selection, and empty-state rendering. No chart
component SHALL reimplement any of these concerns independently.

#### Scenario: A domain-flooring or tick-formatting fix applies everywhere at once
- **WHEN** the axis-domain flooring rule or a tick formatter in the shared kit is changed
- **THEN** every chart composition that renders that kind of axis reflects the change
  without any per-chart code edit

#### Scenario: No chart component defines its own copy of a kit concern
- **WHEN** any chart component under `VEN/ui/src` needs axis flooring, tick formatting,
  tooltip styling, the NOW line, zone shading, sizing constants, or color selection
- **THEN** it obtains that behavior from the shared kit, not from a locally reimplemented
  version

### Requirement: Chart rendering is organized as three compositions over the shared kit, not one universal chart control
`VEN/ui` SHALL provide exactly three chart compositions built on the shared kit —
`TimeSeriesChart` (multi-axis line/step, time X-axis), `StackedTimeSeriesChart` (stacked
areas with net-value tooltip aggregation), and `CurveChart` (non-temporal X-axis) — rather
than a single configurable component covering all chart shapes.

#### Scenario: A time-series chart is expressed as a TimeSeriesChart configuration
- **WHEN** a chart plots one or more series against a time X-axis without stacking (the
  shape used by the former `AssetTimelineChart`, `TariffChart`, `SimProfileChart`,
  `TariffsLineChart`, `TimelineSeriesChart`)
- **THEN** it is implemented as a `TimeSeriesChart` configuration, not a bespoke component

#### Scenario: A stacked chart keeps its own stacking and tooltip aggregation logic
- **WHEN** a chart renders stacked positive/negative series that must be re-aggregated to a
  net value for the tooltip (the shape used by the former `StackedAreaChart`)
- **THEN** it is implemented as `StackedTimeSeriesChart`, reusing the shared kit's
  axis/tick/tooltip-style/NOW-line/zone-shading/sizing/color primitives, while keeping its
  own stacking and net-value aggregation logic

#### Scenario: A non-temporal chart shares only the kit primitives it needs
- **WHEN** a chart's X-axis is not time (the shape used by the former `ComfortCurveChart`)
- **THEN** it is implemented as `CurveChart`, sharing the kit's sizing, tooltip-style, and
  color-registry primitives, without adopting time-domain, NOW-line, or zone-shading logic
  it does not need

### Requirement: A hovered tooltip value always matches the value of the visibly-rendered curve at that point
Every chart composition that renders more than one series SHALL build a single
timestamp-keyed data structure (with forward-fill for sparsely-sampled series) before
rendering, and every rendered series SHALL read its value via an accessor into that one
structure. No composition SHALL render a series from a `data` array independent of the
one used by the other series sharing that hover point.

#### Scenario: Series sampled at different rates still align under the cursor
- **WHEN** a chart renders two series sampled at different rates (e.g. a 1-minute actual
  value and a 5-minute forecast value) and the user hovers a point on the chart
- **THEN** the tooltip's reported value for each series is the value belonging to that
  series at that same timestamp, never a value from a different timestamp in another
  series' own array

#### Scenario: A regression of the index-mismatch bug class is caught by a test, not by visual review
- **WHEN** any chart composition's multi-series tooltip value is checked against the
  shared kit's data-merge builder for a given hovered timestamp
- **THEN** an automated test asserts the reported value equals the merged row's value for
  that series, for every composition that renders more than one series

### Requirement: A Y-axis whose domain spans both negative and positive values always renders a 0.0 tick, anchoring the rest of that axis's ticks
When an axis's resolved domain has a minimum below zero and a maximum above zero, the
shared kit's tick-generation function SHALL include 0.0 as a rendered tick and SHALL
generate the remaining ticks by stepping outward from 0 in both directions, rather than
generating ticks independently of where zero falls in the domain.

#### Scenario: A mixed-sign domain always shows a zero tick
- **WHEN** an axis's resolved domain has `min < 0 < max`
- **THEN** 0.0 is one of the rendered ticks on that axis

#### Scenario: Ticks step outward from zero rather than from the domain start
- **WHEN** an axis's resolved domain has `min < 0 < max` and the tick step size is `s`
- **THEN** the rendered ticks are the set `{0, ±s, ±2s, ...}` intersected with `[min, max]`,
  not a tick set computed by stepping from `min` upward without regard to zero

#### Scenario: A domain that does not straddle zero is unaffected
- **WHEN** an axis's resolved domain is entirely non-negative or entirely non-positive
- **THEN** tick generation is unchanged from the existing (non-zero-anchored) behavior

### Requirement: Decimal precision per physical unit is canonical and applied identically everywhere that unit appears
The shared kit SHALL define one formatting rule per unit, used by every chart's tooltip
and axis tick for that unit:
- Power (kW): magnitude-aware — values with magnitude below 1 kW display in Watts,
  integer-rounded; values at or above 1 kW display in kW to 2 decimal places.
- Cost rate (€/h): 4 decimal places.
- CO₂ rate (g/h): 1 decimal place.
- CO₂ intensity (g/kWh): its own rule, distinct from CO₂ rate, at 3 decimal places.
- Tariff (€/kWh): 4 decimal places.
- State of charge (%): 1 decimal place.
- Temperature (°C): 1 decimal place.

No chart tooltip or axis tick SHALL format a value in one of these units using a
different rule than the one listed above.

#### Scenario: Power tooltips and axis ticks agree with each other
- **WHEN** a chart displays a power value below 1 kW magnitude in both a tooltip and an
  axis tick
- **THEN** both display the value in Watts, integer-rounded, using the same rule

#### Scenario: The same unit is formatted identically regardless of which chart displays it
- **WHEN** two different chart compositions each display a value in the same physical
  unit (e.g. CO₂ rate in g/h)
- **THEN** both apply the same decimal-precision rule for that unit

#### Scenario: A tooltip value's displayed unit label matches its actual physical unit
- **WHEN** a tooltip displays a €/kWh (tariff) or €/h (cost rate) value
- **THEN** the displayed unit label is `€/kWh` or `€/h` respectively, never a bare `€`

### Requirement: One color registry maps series identity to color, replacing the two independent palettes
`VEN/ui` SHALL use a single, ID-keyed color registry for every series across every chart
composition, extended with named keys for tariff/cost/CO₂/grid series in addition to the
existing per-asset keys. No chart SHALL select a series color by positional array index.

#### Scenario: The same series concept renders in the same color in every chart that displays it
- **WHEN** import tariff, export tariff, CO₂ rate, or grid net power is rendered in more
  than one chart composition
- **THEN** it renders in the same color in every one of them

#### Scenario: No chart picks a color by array index
- **WHEN** any chart composition selects a color for a series it renders
- **THEN** it looks up that color by the series' identity key in the shared registry, not
  by a positional index into an array

### Requirement: TariffChart separates tariff (€/kWh), cost rate (€/h), and CO₂ rate (g/h) onto three independently-scaled axes
The chart formerly known as `TariffChart` (now a `TimeSeriesChart` configuration) SHALL
render import/export tariff on one Y-axis scaled and floored independently from the cost
rate series, which SHALL render on its own independently-scaled and floored Y-axis; the
CO₂ rate axis SHALL be unchanged from its current behavior. No two of these three series
groups SHALL share a Y-axis.

#### Scenario: Tariff curves use their own full vertical range
- **WHEN** the chart renders import and export tariff (€/kWh) alongside cost rate (€/h)
- **THEN** the tariff series are scaled against their own axis's domain and floor,
  independent of the cost-rate series' range

#### Scenario: Cost rate no longer shares a scale with tariff
- **WHEN** the chart renders cost rate (€/h)
- **THEN** it is scaled against its own Y-axis, independently floored via the shared
  kit's domain-flooring rule, not the same axis used for tariff

#### Scenario: The CO₂ axis is unaffected
- **WHEN** the chart renders CO₂ rate (g/h)
- **THEN** its axis domain, floor, and position are unchanged from current behavior

#### Scenario: Axis unit labels match the series they carry
- **WHEN** the tariff axis or the cost-rate axis is rendered
- **THEN** its unit label reads `€/kWh` or `€/h` respectively, never a generic `€`

### Requirement: Raw-diagnostics charts use a named, shared height constant distinct from the dashboard-cell height
`SimProfileChart`, `TariffsLineChart`, and `TimelineSeriesChart` (as `TimeSeriesChart`
configurations) SHALL use a shared, named height constant defined once in the sizing
contract, kept visually distinct from (and taller than) the dashboard-cell height used by
Controller/History charts, rather than each independently hardcoding the same literal
value.

#### Scenario: The diagnostic chart height is defined once
- **WHEN** any raw-diagnostics chart is rendered
- **THEN** its height comes from one shared named constant in the sizing contract, not a
  literal duplicated in that chart's own file

#### Scenario: The diagnostic height remains visually unchanged from today
- **WHEN** a raw-diagnostics chart is rendered under the new shared constant
- **THEN** its rendered height is unchanged from its current value
