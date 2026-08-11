## ADDED Requirements

### Requirement: Obligation dropped on confirmed VTN 404
The VEN SHALL remove a report obligation from its active obligation state when the VTN responds
to that obligation's report submission with HTTP 404 Not Found, instead of leaving the
obligation due for another retry cycle.

#### Scenario: VTN 404s a due obligation's report submission
- **WHEN** `ObligationService::check_and_report` submits a due obligation's report and the VTN
  responds with HTTP 404 (its source event or program no longer exists)
- **THEN** the obligation is removed from `AppState`'s report-obligation state and is not present
  in a subsequent `state.report_obligations()` read
- **AND** no further report submission is attempted for that obligation on later ticks

#### Scenario: A 404 on one obligation does not remove sibling obligations of the same event
- **WHEN** two report obligations share the same `event_id` and only one of them receives a 404
  on submission
- **THEN** only the obligation whose own submission 404'd is removed
- **AND** the sibling obligation remains in `AppState`'s report-obligation state, due for its own
  next check

#### Scenario: Non-404 VTN failure still retries unchanged
- **WHEN** `ObligationService::check_and_report` submits a due obligation's report and the VTN
  responds with a non-404 failure (e.g. connection error, 500, 409)
- **THEN** the obligation remains in `AppState`'s report-obligation state with its `due_at`
  unchanged, so it is retried on the next tick
- **AND** this matches current behavior — unaffected by this requirement

### Requirement: HTTP status observable at the obligation-service boundary
The VEN's VTN adapter SHALL preserve the numeric HTTP status of a non-2xx `/reports` response
through its error return value, so that callers above the HTTP boundary can distinguish a 404
from other failure classes without depending on the VTN's error-message text.

#### Scenario: A 404 response is distinguishable from a 409 or 500 response
- **WHEN** the VTN adapter's report-submission call receives a non-2xx HTTP response
- **THEN** the returned error value carries the response's numeric HTTP status
- **AND** a caller can determine whether that status was 404, independent of the response body's
  wording

### Requirement: Correct capacity-reservation report payload-type strings
The VEN SHALL emit `IMPORT_RESERVATION_CAPACITY` and `EXPORT_RESERVATION_CAPACITY` as the
`payloadType`/report-type strings for import and export capacity-reservation obligation reports,
matching openleadr-wire's `ReportType` enum wire representation.

#### Scenario: Import capacity-reservation obligation report uses the correct payload type
- **WHEN** the VEN builds a measurement report for an obligation whose payload type routes to the
  import capacity-reservation case
- **THEN** the built report's payload `type` field is exactly `IMPORT_RESERVATION_CAPACITY`

#### Scenario: Export capacity-reservation obligation report uses the correct payload type
- **WHEN** the VEN builds a measurement report for an obligation whose payload type routes to the
  export capacity-reservation case
- **THEN** the built report's payload `type` field is exactly `EXPORT_RESERVATION_CAPACITY`
