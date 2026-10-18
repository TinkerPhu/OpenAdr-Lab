## ADDED Requirements

### Requirement: Before and after are the fleet's plans around the publish instant
After a draft is published, the workbench SHALL show the fleet's planned import and export power as of
just before the publish instant (the reference) and as of now (the result), both read from the fleet
plan feed (`fleet-plan-feed`), with the publish instant being the published event's `createdDateTime`.

#### Scenario: Reference and result drawn
- **WHEN** a tariff has been published and at least one VEN has replanned
- **THEN** the chart shows the planned import and export from before the publish faintly, and from now
  solid, on the same axis

#### Scenario: Survives a reload
- **WHEN** the page is reloaded after a publish
- **THEN** the same reference and result are shown, because both are read from the stored plans and the
  event's creation time, not from page memory

### Requirement: Replan progress is stated
The workbench SHALL say how many of the targeted VENs now contribute a plan adopted after the publish
instant, and SHALL mark the delta provisional until all of them do.

#### Scenario: Not yet replanned
- **WHEN** 3 of 5 targeted VENs contribute a plan adopted after the publish
- **THEN** the delta is labelled "3 of 5 VENs replanned"

#### Scenario: Settled
- **WHEN** all targeted VENs contribute a plan adopted after the publish
- **THEN** the delta is shown without the provisional label

### Requirement: The delta is stated per direction
The workbench SHALL show, for import and for export separately, the change in planned energy over the
plan horizon and in planned peak power between the reference and the result.

#### Scenario: Energy and peak
- **WHEN** the result plans 12 kWh less evening import and a 4 kW lower peak than the reference
- **THEN** the panel shows -12 kWh and -4 kW for import, and the export figures independently

### Requirement: No plan feed, no comparison
When the fleet plan feed has no plan for the reference instant (a VEN or the store is missing it), the
workbench SHALL say which part is missing rather than draw a zero reference.

#### Scenario: Store unavailable
- **WHEN** `GET /api/fleet/plan` reports no plan history
- **THEN** the comparison panel says so and the rest of the page still works
