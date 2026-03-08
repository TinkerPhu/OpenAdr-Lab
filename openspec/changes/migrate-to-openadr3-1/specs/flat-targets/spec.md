## ADDED Requirements

### Requirement: Targets are a flat array of strings
Program, event, VEN, and resource objects SHALL represent `targets` as a flat JSON array
of strings (`string[]`). The 3.0 `[{type, values}]` object structure MUST NOT be used.

#### Scenario: Program with clientId targets serializes correctly
- **WHEN** a program is created with targets `["ven-1-client", "ven-2-client"]`
- **THEN** `GET /programs/{id}` returns `"targets": ["ven-1-client", "ven-2-client"]`

#### Scenario: Program with empty targets is open to all VENs
- **WHEN** a program is created with `targets: []`
- **THEN** all authenticated VENs can see it via `GET /programs` with `read_targets` scope

### Requirement: Object privacy — VEN sees only its targeted events
When a VEN with `read_targets` scope calls `GET /events`, the VTN SHALL return only events
where the VEN's `client_id` appears in the event's `targets` array, OR the `targets` array
is empty.

#### Scenario: VEN sees event targeted to its clientId
- **GIVEN** an event exists with `targets: ["ven-1-client"]`
- **WHEN** `GET /events` is called with a token for `ven-1-client`
- **THEN** the event appears in the response

#### Scenario: VEN does not see event targeted to another VEN
- **GIVEN** an event exists with `targets: ["ven-2-client"]`
- **WHEN** `GET /events` is called with a token for `ven-1-client`
- **THEN** the event does NOT appear in the response

#### Scenario: VEN sees open event with no targets
- **GIVEN** an event exists with `targets: []`
- **WHEN** `GET /events` is called with any VEN token
- **THEN** the event appears in the response

#### Scenario: bl-client sees all events regardless of targets
- **GIVEN** events exist with various target configurations
- **WHEN** `GET /events` is called with a `read_all` token
- **THEN** all events are returned

### Requirement: Object privacy — VEN sees only its enrolled programs
When a VEN with `read_targets` scope calls `GET /programs`, the VTN SHALL apply the same
clientId-based filtering as for events.

#### Scenario: VEN sees enrolled program
- **GIVEN** a program exists with `targets: ["ven-1-client"]`
- **WHEN** `GET /programs` is called with a token for `ven-1-client`
- **THEN** the program appears in the response

#### Scenario: VEN does not see program targeted to other VENs
- **GIVEN** a program exists with `targets: ["ven-2-client", "ven-3-client"]`
- **WHEN** `GET /programs` is called with a token for `ven-1-client`
- **THEN** the program does NOT appear in the response
