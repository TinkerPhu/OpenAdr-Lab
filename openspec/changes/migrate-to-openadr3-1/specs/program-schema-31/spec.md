## ADDED Requirements

### Requirement: Program request body uses 3.1 schema
The `ProgramRequest` wire format SHALL match the 3.1 spec: fields `programName`,
`intervalPeriod` (optional), `programDescriptions` (optional), `payloadDescriptors` (optional),
`attributes` (optional array of `ValuesMap`), `targets` (flat string array, default `[]`).
The deprecated fields `programType`, `country`, `principalSubdivision`, `bindingEvents`,
`localPrice`, `businessId`, `retailerName`, `retailerLongName`, `programLongName`
MUST NOT be present in request or response bodies.

#### Scenario: Create program with minimal fields
- **WHEN** `POST /programs` is called with `{ "programName": "Test DR", "objectType": "PROGRAM" }`
- **THEN** the response is HTTP 201 with `programName: "Test DR"` and `targets: []`

#### Scenario: Create program with attributes
- **WHEN** `POST /programs` is called with an `attributes` array of `ValuesMap` objects
- **THEN** the created program includes the `attributes` field in the response

#### Scenario: Deprecated fields are rejected or ignored
- **WHEN** `POST /programs` is called with a body including `programType` or `country`
- **THEN** those fields are not present in the stored or returned program object

### Requirement: Program GET response includes 3.1 fields only
`GET /programs` and `GET /programs/{id}` SHALL return programs with only 3.1-compliant fields.
The `objectType` tag SHALL be `"PROGRAM"`.

#### Scenario: GET program returns camelCase fields
- **WHEN** `GET /programs/{id}` is called
- **THEN** the response uses `programName`, `createdDateTime`, `modificationDateTime`, `targets`
- **AND** no snake_case field names appear in the JSON

### Requirement: Programs point at the lab profile through attributes
With `programType` removed in 3.1, the lab profile identifier SHALL be carried as a namespaced,
versioned entry in the program's `attributes`, alongside a published profile document the entry
points at. A spec-only consumer SHALL remain correct without reading it.

#### Scenario: Seeded program declares its profile
- **WHEN** the seed creates a lab program
- **THEN** its `attributes` contains a namespaced, versioned profile entry
- **AND** the entry resolves to the published profile document

#### Scenario: The extension is additive only
- **WHEN** a consumer ignores the profile attribute
- **THEN** the program is still valid and interpretable from its spec fields alone
  <!-- wire-contracts: protocol fields first; extensions namespaced, versioned, additive -->

#### Scenario: Payload descriptors accompany the program
- **WHEN** a program is created
- **THEN** it carries `payloadDescriptors` for every payload type its events use
