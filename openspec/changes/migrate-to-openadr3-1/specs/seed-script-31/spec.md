## ADDED Requirements

### Requirement: Seed script provisions users with scopes
The seed script (`scripts/seed_vtn.py`) SHALL create users and credentials via the VTN API
(or fixture SQL) using the 3.1 scope model. Each VEN user SHALL have scopes
`read_targets, read_ven_objects, write_reports, write_subscriptions`. The bl-client
user is provisioned via fixture SQL.

#### Scenario: Seed creates three VEN users
- **WHEN** the seed script is run
- **THEN** users `ven-1-client`, `ven-2-client`, `ven-3-client` exist in the VTN
- **AND** each can authenticate and obtain a token

### Requirement: Seed script creates programs with flat clientId targets
The seed script SHALL create at least 2 programs: one restricted (clientId targets) and one
open (empty targets). Targets SHALL be flat string arrays of clientIds.

#### Scenario: Restricted program has correct targets
- **WHEN** the seed script runs
- **THEN** "Summer Peak DR" exists with `targets: ["ven-1-client", "ven-2-client"]`

#### Scenario: Open program has empty targets
- **WHEN** the seed script runs
- **THEN** "HVAC Optimization" exists with `targets: []`

### Requirement: Seed script is idempotent
Running the seed script multiple times SHALL NOT create duplicate programs or events.
Existing objects SHALL be detected (by name or ID) and skipped.

#### Scenario: Re-running seed does not duplicate programs
- **WHEN** the seed script is run twice
- **THEN** `GET /programs` returns the same number of programs as after the first run

### Requirement: Seed script creates events linked to programs
The seed script SHALL create at least one event per program with valid 3.1 interval
and payload structures.

#### Scenario: Event is linked to program by programID
- **WHEN** the seed script runs
- **THEN** at least one event exists for each seeded program
- **AND** each event has at least one interval with a payload
