## ADDED Requirements

### Requirement: Report request body uses 3.1 schema
The `ReportRequest` wire format SHALL match the 3.1 spec. Fields: `eventID` (required),
`clientName` (required, 1-128 chars), `reportName` (optional), `payloadDescriptors` (optional),
`resources` (required array). The fields `programId` and `venId` MUST NOT be present in
the request or response.

#### Scenario: VEN submits a valid 3.1 report
- **WHEN** `POST /reports` is called by a VEN with scopes `write_reports` and a body:
  ```json
  {
    "objectType": "REPORT",
    "eventID": "<event-id>",
    "clientName": "ven-1",
    "resources": [{"resourceName": "meter", "intervals": [...]}]
  }
  ```
- **THEN** the response is HTTP 201 with the created report

#### Scenario: Report response includes clientID
- **WHEN** `GET /reports/{id}` is called with a `read_all` token
- **THEN** the response includes `"clientID": "<ven-client-id>"` (set by VTN from token sub)
- **AND** the response does NOT include `programId` or `venId`

### Requirement: VEN app reporter generates 3.1-compliant report payloads
The VEN reporter module SHALL build `ReportRequest` objects without `programId` or `venId`.
The `eventID` field SHALL be taken from the triggering event's `id`. The `clientName` SHALL
be the VEN's `venName` from its profile.

#### Scenario: Reporter omits programId
- **WHEN** the VEN reactor triggers a report for an event
- **THEN** the generated JSON body does not contain a `programId` field

#### Scenario: Reporter sets eventID correctly
- **WHEN** the VEN submits a report for event with ID `"event-001"`
- **THEN** the report body contains `"eventID": "event-001"`
