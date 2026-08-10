## ADDED Requirements

### Requirement: BASELINE report is the event-blind heuristic forecast
When a report obligation requests `payloadType: "BASELINE"`, the VEN SHALL build the report
payload from the heuristic forecast (`AssetHeuristics::sample_kw`) for the obligation's
requested interval(s), evaluated without regard to any currently active event. The VEN SHALL
NOT substitute the plan's actual controlled setpoint or any event-adjusted value.

#### Scenario: BASELINE requested during an active capacity-limit event
- **WHEN** a report obligation with `payloadType: "BASELINE"` is due while an
  `IMPORT_CAPACITY_LIMIT` event is active and constraining the plan
- **THEN** the BASELINE report's value for each interval equals the heuristic's
  `sample_kw` for that interval's start time, not the plan's (event-constrained) actual
  net site power

#### Scenario: BASELINE requested with no active event
- **WHEN** a report obligation with `payloadType: "BASELINE"` is due and no event is active
- **THEN** the BASELINE report's value still equals the heuristic's `sample_kw` for each
  requested interval (the computation does not depend on event state at all)

### Requirement: BASELINE reports are obligation-driven only
The VEN SHALL submit a `BASELINE` report only in response to an explicit report obligation
requesting `payloadType: "BASELINE"`. The VEN SHALL NOT emit BASELINE reports automatically
alongside timer-driven `USAGE` reports for events that carry no such obligation.

#### Scenario: Event active with no BASELINE obligation
- **WHEN** an event is active and being measured via the timer-driven `USAGE` report path
  (no `reportDescriptors` requesting `payloadType: "BASELINE"`)
- **THEN** no BASELINE report is submitted for that event

### Requirement: BASELINE payloads carry a quality/provenance tag
Each BASELINE report interval SHALL include a quality-tag payload entry whose value is the
`ForecastSource` variant of the heuristic used to compute it (e.g. `"HEURISTIC"`).

#### Scenario: Heuristic-sourced baseline includes its provenance tag
- **WHEN** a BASELINE report is built from `AssetHeuristics::sample_kw`
- **THEN** each interval's payloads include a quality-tag entry with value `"HEURISTIC"`,
  alongside the BASELINE value itself

### Requirement: Experiment KPI evaluation quantifies event impact from BASELINE/USAGE pairs
`experiments/kpi.py` SHALL compute `event_impact_kwh` for an event as the sum, across that
event's reporting window, of `(baseline_kw − actual_kw) × interval_hours`, using the
recorder's archived `BASELINE` and `USAGE` report rows for the same event/window.

#### Scenario: BASELINE above actual (event reduced consumption)
- **WHEN** an event's archived BASELINE reports total more energy than its archived USAGE
  reports over the same window
- **THEN** `event_impact_kwh` for that event is positive, equal to the energy difference

#### Scenario: No BASELINE reports archived for an event
- **WHEN** an event has archived USAGE reports but no archived BASELINE reports for the same
  window
- **THEN** `event_impact_kwh` for that event is `None`/absent rather than a computed value
