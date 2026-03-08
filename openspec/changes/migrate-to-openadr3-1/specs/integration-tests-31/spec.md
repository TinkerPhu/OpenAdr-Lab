## ADDED Requirements

### Requirement: Provisioning BDD scenarios use 3.1 auth model
The integration test provisioning steps SHALL use the scope-based auth model.
Token acquisition SHALL use credential pairs corresponding to `bl-client` or per-VEN credentials.
Old steps referencing user roles (business, ven-manager, user-manager) SHALL be updated.

#### Scenario: Provisioning step obtains bl-client token
- **WHEN** a BDD step runs "I authenticate as the business layer client"
- **THEN** `POST /auth/token` is called with `bl-client` / `bl-client`
- **AND** subsequent API calls use the returned token

#### Scenario: Provisioning step obtains VEN token
- **WHEN** a BDD step runs "I authenticate as VEN ven-1"
- **THEN** `POST /auth/token` is called with `ven-1-client` / `ven-1-client`

### Requirement: Enrollment BDD scenarios use flat targets
Integration test scenarios that verify program enrollment SHALL use flat string target
arrays, not the 3.0 `{type, values}` structure.

#### Scenario: Enrollment scenario creates program with clientId targets
- **WHEN** a BDD step creates a program with targets `["ven-1-client"]`
- **THEN** the program body sent to the VTN is `"targets": ["ven-1-client"]`

#### Scenario: Enrollment scenario verifies VEN visibility
- **WHEN** `GET /programs` is called with `ven-1-client` token
- **THEN** only programs where `ven-1-client` is in targets (or targets is empty) are returned

### Requirement: Report BDD scenarios assert 3.1 fields
Integration test steps that inspect submitted reports SHALL assert `eventID` and `clientName`
presence, and SHALL assert absence of `programId` and `venId`.

#### Scenario: Report assertion checks eventID not programId
- **WHEN** a BDD step validates a submitted report
- **THEN** the step checks `eventID` is present and equals the triggering event's ID
- **AND** the step does NOT assert `programId`

### Requirement: VEN self-registration is tested
An integration scenario SHALL verify that each VEN can self-register against the VTN.

#### Scenario: VEN self-registration scenario
- **GIVEN** user `ven-1-client` exists with VEN scopes
- **WHEN** `POST /vens` is called with a VEN token and body `{ "objectType": "VEN_VEN_REQUEST", "venName": "ven-1" }`
- **THEN** the response is HTTP 201 with `clientID: "ven-1-client"`
