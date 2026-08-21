## ADDED Requirements

### Requirement: Dedicated capacity-forecast chart
The VEN UI SHALL provide a chart, distinct from `SiteHeadroomChart`, that renders both
direction curves (sustained-import-commitment, sustained-export-commitment) as power vs.
elapsed-time-since-commitment, including each curve's step points and its cumulative energy total.
`SiteHeadroomChart` SHALL remain unchanged and continue to show only the instantaneous envelope.

#### Scenario: Both directions visible
- **WHEN** the capacity-forecast chart renders
- **THEN** it shows the import-commitment curve and the export-commitment curve as two distinct
  series, each with its own cumulative energy total displayed

#### Scenario: Step points are visible, not smoothed
- **WHEN** an asset's contribution steps down (e.g. a battery reaches `min_soc`, a shiftable load
  is placed)
- **THEN** the chart renders that discontinuity as a step, not an interpolated slope

### Requirement: Surfaced under Diagnostics, not the main Dashboard
The new chart SHALL be placed under the VEN UI's Diagnostics menu group, consistent with other
derived/advanced-metric surfaces, not on the main Dashboard.

#### Scenario: Chart reachable from Diagnostics
- **WHEN** a user navigates to VEN UI Diagnostics
- **THEN** the capacity-forecast chart is present as one of the diagnostics entries

### Requirement: Backend capability has a visible UI surface
Per this project's UI-transparency rule, the capacity-curve computation SHALL NOT ship without a
corresponding UI surface in the same piece of work — a read route with no chart consuming it is an
incomplete implementation.

#### Scenario: Route and chart shipped together
- **WHEN** the capacity-forecast read route is deployed
- **THEN** the Diagnostics chart consuming that route is deployed in the same change, not deferred
