## ADDED Requirements

### Requirement: Event request body uses the 3.1 schema
The event wire format SHALL match 3.1: `EventContent` becomes `EventRequest`, `targets` is a
non-optional flat string array defaulting to `[]`, `duration` is a new optional top-level field,
and `intervals` is **optional**.

#### Scenario: Event with duration and no intervals round-trips
- **WHEN** an event is created with `duration: "PT1H"` and no `intervals`
- **THEN** it is accepted
- **AND** `GET` returns it with `duration` present and no `intervals`

#### Scenario: Event targets serialize flat
- **WHEN** an event targeting one VEN is read
- **THEN** `targets` is `["ven-1"]`, not `[{"type":"VEN_NAME","values":["ven-1"]}]`

### Requirement: One authority answers when an event interval runs
The rule deciding an event's active window and each interval's start/end SHALL have exactly one
implementation in the VEN, in `VEN/src/controller/openadr_interface.rs`, and every caller SHALL
use it. The copies in `VEN/src/controller/report_intervals.rs` and any other parser SHALL be
deleted in the same piece of work.

#### Scenario: Duration-only event yields a window
- **GIVEN** an event with `intervalPeriod.start`, `duration` and no `intervals`
- **WHEN** the VEN determines the event's window
- **THEN** it is `start … start + duration`

#### Scenario: Per-interval timing still resolves
- **GIVEN** an event with no event-level `intervalPeriod` but per-interval periods
- **WHEN** the VEN determines the window
- **THEN** it ends when the last interval ends

#### Scenario: An undeterminable window is surfaced, not guessed
- **GIVEN** an event whose window cannot be determined from what it declares
- **WHEN** the VEN processes it
- **THEN** the VEN records that it could not place the interval
- **AND** it does not substitute an assumed duration
  <!-- wire-contracts: a silent assumption is a bug, not a default -->

#### Scenario: No second copy of the rule exists
- **WHEN** the VEN source is searched for interval-window logic
- **THEN** only the single authority computes it
  <!-- GB-48: this concept previously lived in seven parsers with four rules -->

### Requirement: Events we emit declare their payload semantics
Every event the lab creates SHALL carry `payloadDescriptors` declaring payload type, units and,
where applicable, currency, for every payload type appearing in its intervals.

#### Scenario: Created event carries descriptors
- **WHEN** the seed creates an event with `IMPORT_CAPACITY_LIMIT` payloads
- **THEN** the event's `payloadDescriptors` declares that payload type with unit `KW`

#### Scenario: An undeclared payload is rejected at the boundary
- **WHEN** an event is constructed with a payload type absent from its `payloadDescriptors`
- **THEN** construction fails
  <!-- wire-contracts: make it structural -->
