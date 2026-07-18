## ADDED Requirements

### Requirement: Record report submission outcomes
The VEN SHALL record the outcome (accepted or rejected) of every report
submitted via `POST /reports` or `PUT /reports/:id`, in a bounded in-memory
ring, independent of the `reports_sent_total` metrics counter.

#### Scenario: Successful submission is recorded as accepted
- **WHEN** `POST /reports` is called and the VTN accepts the report
- **THEN** a submission record is stored with `vtnAccepted: true`, the
  submitted `reportName`/`eventID`/`clientName`, and a `submittedAt`
  timestamp

#### Scenario: Failed submission is recorded as rejected
- **WHEN** `POST /reports` or `PUT /reports/:id` is called and the VTN
  returns an error
- **THEN** a submission record is stored with `vtnAccepted: false` and a
  non-null `error` field describing the failure

#### Scenario: Ring is bounded
- **WHEN** more than 100 report submissions have been recorded
- **THEN** only the 100 most recent submission records are retained

### Requirement: Expose report submission outcomes via a dedicated route
The VEN SHALL expose recorded report submission outcomes via
`GET /reports/submissions`, newest first, without altering the existing
`GET /reports` response shape.

#### Scenario: Submissions list reflects recent attempts
- **WHEN** a client calls `GET /reports/submissions` after one or more
  report submissions have occurred
- **THEN** the response is a JSON array of submission records ordered
  newest-first

#### Scenario: Empty when no submissions have occurred
- **WHEN** a client calls `GET /reports/submissions` before any report has
  ever been submitted via this VEN process
- **THEN** the response is an empty JSON array

### Requirement: Reports page surfaces per-report submission status
The VEN UI's Reports page SHALL display a status chip on each report row
indicating whether the most recent submission attempt for that report
(matched by `reportName`, falling back to `eventID`) was accepted, rejected,
or not yet attempted this session.

#### Scenario: Accepted submission shows an accepted chip
- **WHEN** the Reports page loads and a report row's `reportName` matches a
  submission record with `vtnAccepted: true`
- **THEN** that row displays an "Accepted" status chip

#### Scenario: Rejected submission shows a rejected chip
- **WHEN** the Reports page loads and a report row's `reportName` matches a
  submission record with `vtnAccepted: false`
- **THEN** that row displays a "Rejected" status chip

#### Scenario: No matching submission shows no chip
- **WHEN** a report row has no matching submission record
- **THEN** that row displays no status chip (or a neutral placeholder, not a
  false negative)
