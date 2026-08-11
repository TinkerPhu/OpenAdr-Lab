## ADDED Requirements

### Requirement: Every real report submission is recorded in submission history
The VEN SHALL append a `ReportSent` history row via `HistoryPort::append_report_sent` for every
report submission it makes to the VTN through a real (non-test) call path, whenever a
`HistoryPort` is configured.

#### Scenario: Timer-driven measurement report submission is recorded
- **WHEN** `tasks/sim_tick/publish.rs::run_measurement_reports` submits a measurement report for
  an active event to the VTN
- **THEN** a `ReportSent` row for that submission is appended through `HistoryPort`

#### Scenario: Obligation-driven report submission is recorded
- **WHEN** `services/obligation.rs::check_and_report` submits a due obligation's report to the
  VTN
- **THEN** a `ReportSent` row for that submission is appended through `HistoryPort`

#### Scenario: REST-initiated report submission is recorded
- **WHEN** a client submits a report via `POST /reports` or `PUT /reports/{id}`
  (`routes/reports.rs`) and the VTN accepts it
- **THEN** a `ReportSent` row for that submission is appended through `HistoryPort`

#### Scenario: No HistoryPort configured — submission still succeeds
- **WHEN** any of the above report-submission paths runs with no `HistoryPort` configured (e.g.
  `history: None`)
- **THEN** the report submission to the VTN proceeds and succeeds/fails exactly as it would with
  a `HistoryPort` configured
- **AND** no error is raised or logged solely due to the absence of a `HistoryPort`

### Requirement: Submission history is queryable and reflects real submissions
`GET /history/reports` SHALL return the `ReportSent` rows recorded via the requirement above,
so the endpoint reflects actual VEN report-submission activity rather than always being empty.

#### Scenario: A real submission becomes visible through the history endpoint
- **WHEN** a report submission recorded per the "Every real report submission is recorded"
  requirement completes
- **THEN** a subsequent `GET /history/reports` call returns a row corresponding to that
  submission
