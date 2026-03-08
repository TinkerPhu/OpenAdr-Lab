## ADDED Requirements

### Requirement: Program form reflects 3.1 schema
The VTN UI program creation and edit form SHALL include only 3.1-compliant fields:
`programName`, `intervalPeriod` (optional), `attributes` (optional). The deprecated fields
`programType`, `country`, `principalSubdivision`, `bindingEvents`, `localPrice`,
`businessId`, `retailerName` SHALL be removed from the form.

#### Scenario: Program form does not show deprecated fields
- **WHEN** the user opens the program creation form in the VTN UI
- **THEN** no fields for `programType`, `country`, `bindingEvents`, or `retailerName` are visible

#### Scenario: Program form shows attributes field
- **WHEN** the user opens the program creation form
- **THEN** an optional `attributes` field is available (e.g., as a JSON editor or key-value list)

### Requirement: VEN list shows clientID
The VEN list page in the VTN UI SHALL display each VEN's `clientID` alongside its `venName`.

#### Scenario: VEN list shows clientID column
- **WHEN** the user navigates to the VENs page
- **THEN** each VEN row shows both `venName` and `clientID`

### Requirement: Enrollment display uses flat targets
The VTN UI program detail or enrollment display SHALL show the flat `targets` array
(list of clientIds) rather than the 3.0 `{type, values}` objects.

#### Scenario: Program targets are displayed as a flat list
- **WHEN** the user views a program's details
- **THEN** enrolled clients are shown as a flat list of client ID strings

### Requirement: Reports page does not show programId
The VTN UI reports listing SHALL NOT display a `programId` column or field, as it no longer
exists in the 3.1 report schema.

#### Scenario: Reports page shows eventID instead of programId
- **WHEN** the user navigates to the Reports page
- **THEN** each report row shows `eventID`
- **AND** no `programId` field is visible
