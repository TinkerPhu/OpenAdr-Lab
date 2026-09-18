## ADDED Requirements

### Requirement: Report request body uses 3.1 schema
The `ReportRequest` wire format SHALL match the 3.1 spec. Fields: `eventID` (required),
`clientName` (required, 1-128 chars), `reportName` (optional), `payloadDescriptors` (optional),
`resources` (required array). The fields `programID` and `venID` MUST NOT be present in
the request or response.

#### Scenario: VEN submits a valid 3.1 report
- **WHEN** `POST /reports` is called by a VEN with scopes `write_reports_ven` and a body:
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
- **AND** the response does NOT include `programID` or `venID`

### Requirement: VEN app reporter generates 3.1-compliant report payloads
The VEN reporter module SHALL build `ReportRequest` objects without `programID` or `venID`.
The `eventID` field SHALL be taken from the triggering event's `id`. The `clientName` SHALL
be the VEN's `venName` from its profile.

#### Scenario: Reporter omits programId
- **WHEN** the VEN controller triggers a report for an event
- **THEN** the generated JSON body does not contain a `programID` field

#### Scenario: Reporter sets eventID correctly
- **WHEN** the VEN submits a report for event with ID `"event-001"`
- **THEN** the report body contains `"eventID": "event-001"`

### Requirement: Report descriptors declare their interval semantics and units
Every `ReportDescriptor` the lab emits SHALL set `reportIntervals`
(`INTERVALS | SUB_INTERVALS | OPEN_INTERVALS`) explicitly, and every `ReportPayloadDescriptor`
SHALL declare `payloadType`, `readingType` and `units`. The `INTERVALS` default SHALL NOT be
inherited by omission.

#### Scenario: Emitted descriptor is fully declared
- **WHEN** the VEN registers a report descriptor
- **THEN** the body carries an explicit `reportIntervals` value
- **AND** its payload descriptors carry `payloadType`, `readingType` and `units`

#### Scenario: Power is declared in kW and energy in kWh
- **WHEN** a `DEMAND` payload is reported
- **THEN** its descriptor declares `units: "KW"` and the values are kilowatts
- **AND** a `USAGE` payload declares `units: "KWH"` and the values are kilowatt-hours
  <!-- GB-50; the Unit enum has no watt in either 3.0 or 3.1 -->

#### Scenario: An undeclared payload cannot be sent
- **WHEN** a report is constructed with a payload type absent from its payload descriptors
- **THEN** construction fails
  <!-- wire-contracts: make it structural -->

### Requirement: Every report names the event it belongs to
`eventID` SHALL be present on every report the VEN submits; 3.1 removes `programID`, so there is
no program-level reporting.

#### Scenario: Report without an event is rejected at the boundary
- **WHEN** a report body is built with no `eventID`
- **THEN** it is rejected before submission
