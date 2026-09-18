## ADDED Requirements

### Requirement: The active-event filter survives the rebase, pagination bug included
The rebased VTN SHALL support `GET /events?active=true|false`, with the active predicate applied
**inside** the same SQL query that applies `OFFSET`/`LIMIT`. The single authority for an event's
end instant SHALL be `EventRequest::ends_at()` in `openleadr-wire`, and it SHALL account for
3.1's top-level `duration` and optional `intervals`.

#### Scenario: Active filter combined with pagination returns a correct page
- **GIVEN** a mix of active and past events exceeding one page
- **WHEN** `GET /events?active=true&skip=0&limit=1` is called
- **THEN** exactly one event is returned, and it is active
- **AND** the page is neither short nor drawn from the unfiltered ordering
  <!-- regression for the original bug: LIMIT/OFFSET ran before the Rust-side filter -->

#### Scenario: Event with a top-level duration and no intervals
- **GIVEN** a 3.1 event with `duration` set and `intervals` absent
- **WHEN** its end instant is computed
- **THEN** it is `intervalPeriod.start + duration`
- **AND** the event is treated as active until that instant

#### Scenario: Open-ended event is always active
- **GIVEN** an event with no determinable end
- **WHEN** `GET /events?active=true` is called
- **THEN** the event is included

#### Scenario: ends_at stays in sync
- **WHEN** an event is created or updated
- **THEN** its stored `ends_at` matches `EventRequest::ends_at()` for the new content

### Requirement: Deleting an event deletes its reports
The `report.event_id` foreign key SHALL be declared `ON DELETE CASCADE`.

#### Scenario: Event with submitted reports can be deleted
- **GIVEN** a VEN has submitted a report against an event
- **WHEN** `DELETE /events/{id}` is called by a client with `write_events`
- **THEN** the response is success, not HTTP 409
- **AND** the associated reports no longer exist

### Requirement: The VTN image builds with the internal OAuth provider
The `vtn.Dockerfile` SHALL build `openleadr-vtn` with the `internal-oauth` feature enabled, and
SHALL retain the multi-stage cargo-chef + BuildKit cache-mount structure.

#### Scenario: Token endpoint exists in the built image
- **WHEN** the VTN container built from `vtn.Dockerfile` starts
- **AND** `POST /auth/token` is called with valid credentials
- **THEN** the response is HTTP 200 with a Bearer token, not HTTP 404

#### Scenario: Incremental rebuild reuses cached dependencies
- **GIVEN** a previous build exists and only application source changed
- **WHEN** the image is rebuilt
- **THEN** dependency compilation is served from cache rather than recompiled

### Requirement: Target-hiding privacy holds under the native 3.1 implementation
The lab-specific `strip_ven_name_targets` / VEN_NAME reconstruction code SHALL be removed, and
the privacy property it protected SHALL be asserted against 3.1's native target hiding.

#### Scenario: A VEN sees only its own target in a program
- **GIVEN** a program targeting `["ven-1", "ven-2"]`
- **WHEN** `ven-1` reads that program
- **THEN** the returned `targets` contains `"ven-1"` only

#### Scenario: A VEN not targeted cannot read the object
- **WHEN** `ven-3` requests an event targeting `["ven-1"]`
- **THEN** the response is HTTP 404

#### Scenario: An untargeted object is visible to every VEN
- **GIVEN** an event with `targets: []`
- **WHEN** any VEN reads it
- **THEN** it is returned

#### Scenario: A business client sees full targets
- **WHEN** a client with `read_all` reads the same program
- **THEN** the returned `targets` contains every targeted clientId
