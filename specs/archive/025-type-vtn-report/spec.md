# Feature Specification: Type the VTN Report Interface

**Feature Branch**: `025-type-vtn-report`  
**Created**: 2026-05-14  
**Status**: Draft  
**Phase**: 7 of 7 — final structural gap from `docs/plans/ven_backend_architecture_refactoring.md`

## Background

Phases 1–6 (AB-01 through AB-06) introduced a clean layered architecture for the VEN backend, culminating in the `VtnPort` trait (Phase 6 / `024-arch-gaps-complete`). One deliberate deferral remained: the report body passed to `VtnPort::upsert_report` is still an untyped JSON blob. This phase eliminates that final gap.

Four public report-building functions in the reporter module currently construct OpenADR report payloads using raw JSON macros and return an untyped value. All three callers (the planning task, the sim-tick publish task, and the obligation service) pass those values straight through to the VTN port without any compile-time shape guarantee.

This feature replaces those untyped boundaries with four typed structs that mirror the OpenADR 3 report shape, updates the port signature to accept the typed body and return a plain success/failure result, and aligns all callers and the mock implementation accordingly.

---

## Clarifications

### Session 2026-05-14

- Q: What should `post_reports` return to its HTTP caller on success after `upsert_report` returns `Result<()`? → A: Return `201 Created` with the submitted `OadrReportBody` as the response body (echo-back).
- Q: Should the `OadrReportBody` round-trip contract test use structural JSON equality or exact byte-string comparison? → A: Structural equality — compare two parsed JSON value objects with `assert_eq!`.
- Q: Should `MockVtn::submitted()` return `Vec<OadrReportBody>` (typed) or `Vec<serde_json::Value>` (serialize on exit)? → A: Return `Vec<OadrReportBody>` — typed field access; existing internal mock tests updated accordingly.

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Developer adds a new report payload type (Priority: P1)

A developer extending the VEN to report a new OpenADR payload type (e.g., `EXPORT_CAPACITY_RESERVATION`) opens `reporter.rs`, constructs an `OadrReportBody` using the typed structs, and passes it to `vtn.upsert_report(body)`. The compiler immediately catches any missing required field or wrong type in the payload — no need to run the code against a live VTN to discover a serialization error.

**Why this priority**: This is the core value of the feature. A fully typed boundary means mistakes surface at compile time, not at runtime in a live OpenADR session.

**Independent Test**: Run `cargo check -p ven` — if the crate compiles with all required struct fields filled, the serialized JSON shape is guaranteed correct. No additional runtime verification needed for this story.

**Acceptance Scenarios**:

1. **Given** a new report type needs to be added, **When** a developer constructs `OadrReportBody` with a missing required field (e.g., omitting `programID`), **Then** the code does not compile.
2. **Given** a correctly constructed `OadrReportBody`, **When** it is serialized to JSON, **Then** the output matches the OpenADR 3 report wire format exactly (field names, nesting, and value types).
3. **Given** the full existing test suite, **When** the typing changes are applied, **Then** all tests remain green with no logic changes.

---

### User Story 2 — Developer inspects a captured report in tests (Priority: P2)

A developer writing a unit test for the obligation service or planning task needs to assert on the body of the report that was submitted to the VTN. Today they must index into a raw JSON value (`body["resources"][0]["intervals"][0]["payloads"][0]["type"]`). After this change they access typed fields directly (`body.resources[0].intervals[0].payloads[0].r#type`).

**Why this priority**: Typed test assertions are less fragile and self-documenting. Wrong field names silently return `null` on untyped JSON but fail to compile on typed structs.

**Independent Test**: Verified by updating existing mock-based tests in `services/obligation.rs` and `services/test_support/mock_vtn.rs` to use field access instead of JSON indexing, and confirming they compile and pass.

**Acceptance Scenarios**:

1. **Given** a `MockVtn` that captures submitted report bodies, **When** a test constructs and submits an `OadrReportBody`, **Then** the test can assert on `.resources`, `.intervals`, `.payloads` fields without JSON indexing.
2. **Given** an `OadrReportBody` round-tripped through `serde_json::to_value` then `serde_json::from_value`, **Then** all field values are preserved exactly.

---

### User Story 3 — Operator submits a report via the VEN HTTP API (Priority: P3)

An operator or integration tool POSTs a report body to the VEN's `/reports` endpoint. The VEN deserializes the JSON directly into an `OadrReportBody` struct, validating required fields on ingestion rather than passing a raw blob downstream.

**Why this priority**: Correctness benefit at the ingestion boundary. Missing required fields now return a 422 Unprocessable Entity response instead of being silently forwarded as malformed JSON.

**Independent Test**: Send a POST to `/reports` with a body missing the required `programID` field; expect a 4xx error response.

**Acceptance Scenarios**:

1. **Given** a POST to `/reports` with a valid OpenADR report body, **When** the request is received and forwarded to the VTN successfully, **Then** the VEN returns `201 Created` with the submitted report body echoed back (field values unchanged).
2. **Given** a POST to `/reports` with a missing `programID`, **When** the request is received, **Then** the VEN returns a 422 error before touching the VTN.

---

### Edge Cases

- A `OadrReportPayload::values` array may contain numbers, strings, or a mix — the `Vec<serde_json::Value>` element type is intentional and must be preserved.
- `build_status_report` produces a report with no `eventID` — the `OadrReportBody.eventID` field is `Option<String>` and must serialize as absent (not `"eventID": null`).
- `upsert_report` returning `Result<()>` means callers that currently inspect the response body (there are none in the current codebase, confirmed by grep) remain unaffected.
- The `update_report` inherent method on `VtnClient` (called only from `routes/reports.rs PUT` handler) is not on the `VtnPort` trait and is explicitly out of scope.

---

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST define four typed structs — `OadrReportBody`, `OadrReportResource`, `OadrReportInterval`, `OadrReportPayload` — that collectively represent the full OpenADR 3 report body shape currently produced by the reporter module.
- **FR-002**: Field names in the new structs MUST match OpenADR 3 wire names verbatim (e.g., `programID`, `clientName`, `reportName`, `resourceName`, `intervalPeriod`), consistent with the camelCase convention already used in `OadrEvent` and `OadrProgram`.
- **FR-003**: The `VtnPort::upsert_report` method signature MUST accept `OadrReportBody` and return `Result<()>`.
- **FR-004**: All four public report-building functions in the reporter module MUST return typed values (`Option<OadrReportBody>` or `Vec<OadrReportBody>`) rather than untyped JSON.
- **FR-005**: The `VtnClient` concrete implementation of `upsert_report` MUST serialize `OadrReportBody` to JSON internally before posting to the VTN HTTP endpoint; the HTTP transport layer is unchanged.
- **FR-006**: The `MockVtn` used in unit tests MUST implement the updated `VtnPort` trait signature, storing received `OadrReportBody` values internally. Its `submitted()` accessor MUST return `Vec<OadrReportBody>` so test assertions can use typed field access. Existing internal mock tests MUST be updated from JSON indexing to struct field access.
- **FR-007**: The VEN's HTTP POST `/reports` handler MUST deserialize the incoming request body directly into `OadrReportBody` rather than accepting a raw JSON blob, and MUST return `201 Created` with the submitted `OadrReportBody` echoed back as the response body on success.
- **FR-008**: The `OadrReportPayload::values` field MUST remain `Vec<serde_json::Value>` to accommodate the heterogeneous value arrays defined by the OpenADR 3 specification.
- **FR-009**: The `OadrReportBody.eventID` field MUST be serialized as absent (not null) when `None`, using `#[serde(skip_serializing_if = "Option::is_none")]`.
- **FR-010**: All new structs MUST derive `Debug`, `Clone`, `Serialize`, `Deserialize`.
- **FR-011**: Contract tests MUST verify that `OadrReportBody` round-trips through JSON serialization/deserialization with all field values preserved.
- **FR-012**: At least one unit test per public reporter function MUST assert on struct fields directly (not via JSON indexing).

### Key Entities

- **OadrReportBody**: Top-level report submission envelope. Fields: `programID` (required), `eventID` (optional), `clientName` (required), `reportName` (required), `resources` (required, list).
- **OadrReportResource**: A single named resource within a report. Fields: `resourceName` (required), `intervals` (required, list).
- **OadrReportInterval**: A single measurement interval. Fields: `id` (required, integer index), `intervalPeriod` (optional, reuses existing `OadrIntervalPeriod` type), `payloads` (required, list).
- **OadrReportPayload**: A single payload value within an interval. Fields: `type` (required, e.g., `"USAGE"`, `"TELEMETRY_STATUS"`), `values` (required, heterogeneous array).

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Zero `serde_json::Value` occurrences on public function signatures in `vtn_port.rs` (trait methods), `reporter.rs` (public functions), and the `VtnPort` impl in `vtn.rs` — verified by grep.
- **SC-002**: All existing automated tests pass without modification to test logic (only type signatures and field access patterns change).
- **SC-003**: The `OadrReportBody` contract test confirms that a fixture round-trip (serialize to JSON value → deserialize back to struct → re-serialize to JSON value) produces structurally equal JSON values (`assert_eq!(v1, v2)`), guaranteeing no field is silently dropped or renamed during serialization.
- **SC-004**: Every public reporter function has at least one test that accesses struct fields by name, eliminating the risk of silent `null` returns from string-keyed JSON indexing.

---

## Assumptions

- No logic changes are made; this is a pure structural typing pass. Existing behaviour — which reports are built, when they are submitted, and what values they contain — is identical before and after.
- The `VtnClient::update_report` inherent method (used by the PUT `/reports` passthrough endpoint) is deliberately excluded from this change; it is not on `VtnPort` and typing it would be a separate task.
- `OadrReportPayload::values` as `Vec<serde_json::Value>` is a deliberate design decision, not a gap: the OpenADR 3 spec defines `values` as a heterogeneous array (numbers for power measurements, strings for state/SoC values). This is internal structure, not a port boundary.
- The `submit_report` method on `VtnClient`, if it becomes unreachable after the refactor, is removed. If it remains reachable (e.g., from a non-trait call site), its signature is updated identically to `upsert_report`.

---

## Out of Scope

- `VtnClient::update_report` typing
- Internal `get_json` / `post_json` / `put_json` helpers in `vtn.rs`
- Any new HTTP routes or behaviour changes
- Changes to the VTN side (openleadr-rs submodule)
