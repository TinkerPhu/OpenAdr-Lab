## ADDED Requirements

### Requirement: VEN object carries mandatory clientID field
Every VEN object in the VTN SHALL have a `clientID` field that equals the OAuth `client_id`
of the VEN's credential. This links the VEN's identity to its OAuth token.

#### Scenario: VEN object includes clientID in GET response
- **WHEN** `GET /vens/{id}` is called with a `read_all` token
- **THEN** the response JSON includes `"clientID": "<ven-client-id>"`

#### Scenario: Duplicate clientID is rejected
- **WHEN** a second `POST /vens` is submitted with the same `clientID`
- **THEN** the response is HTTP 409 Conflict

### Requirement: VEN self-registers on first startup
On startup, each VEN instance SHALL call `POST /vens` with a `VenVenRequest`
(body: `{ "objectType": "VEN_VEN_REQUEST", "venName": "<name>" }`).
The VTN derives `clientID` from the token's `sub` claim and stores it on the VEN object.

#### Scenario: VEN registers itself successfully
- **WHEN** the VEN starts for the first time
- **THEN** it calls `POST /vens` with its own token and `venName`
- **AND** the VTN responds HTTP 201 with a VEN object containing `clientID` equal to the VEN's `client_id`

#### Scenario: VEN handles already-registered case gracefully
- **WHEN** the VEN starts and `POST /vens` returns HTTP 409
- **THEN** the VEN logs at INFO level and continues normal operation (polls programs and events)

### Requirement: VEN YAML profile includes client_id
Each per-VEN YAML profile (`VEN/profiles/ven-{1,2,3}.yaml`) SHALL include a `client_id` field
that matches the OAuth credential client_id used for authentication.

#### Scenario: VEN uses profile client_id for authentication
- **WHEN** the VEN container starts with `PROFILE_PATH` env var set
- **THEN** the VEN reads `client_id` from the profile and uses it to obtain a token

#### Scenario: Profile client_id matches VEN object clientID
- **WHEN** the VEN has self-registered
- **THEN** `GET /vens` (with read_all token) shows a VEN with `clientID` matching the profile `client_id`
