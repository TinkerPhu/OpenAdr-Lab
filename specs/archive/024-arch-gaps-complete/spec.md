# Feature Specification: Complete VEN Architecture — Services Layer, Typed VTN, tick.rs Fix

**Feature Branch**: `024-arch-gaps-complete`  
**Created**: 2026-05-14  
**Status**: Draft  
**Input**: User description: "specify docs/plans/arch-gaps-speckit-input.md"

## Clarifications

### Session 2026-05-14

- Q: Should typed VTN structs model only currently-consumed fields or the full OpenADR 3 schema? → A: Minimal — only fields currently consumed. The project targets OpenADR 3 but the OpenADR 3.1 spec introduces breaking changes to type and field names; minimizing coupling to the current schema reduces future migration cost.
- Q: Should ObligationService retry on VTN error, or propagate and let the task loop retry? → A: No retry inside the service — propagate the error; the obligation task loop retries naturally on its next scheduled tick.

---

## Background

The VEN backend has completed five of seven planned architecture phases. Three gaps remain that
prevent the system from meeting its own declared structural invariants:

1. **No application service layer** (Phase 5) — business logic is embedded in task loops and route handlers, making it untestable without a full runtime.
2. **Untyped VTN communication** (Phase 7) — the VEN communicates with the VTN using raw untyped JSON throughout, preventing compile-time safety and mock-based testing.
3. **tick.rs exceeds the 200-line module limit** — the simulator tick file is over the declared hard limit, violating the project's own maintainability rule.

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Developer tests planning logic without a running simulator (Priority: P1)

A developer needs to verify that the plan acceptance gate correctly rejects a new plan when it
does not improve total cost above the threshold. Today this is impossible without spawning the
full planning loop with a live simulator, real tariff data, and a running HiGHS solver process.

After this feature, the developer creates a mock simulator and mock solver, constructs a
`PlanningService` with those mocks, calls `run_planning_cycle`, and asserts that the plan was
not adopted — all in a unit test that completes in milliseconds.

**Why this priority**: Business rules in the planning loop (acceptance gate, trigger
classification, plan adoption) are the highest-value logic in the VEN. Any regression there
directly affects energy cost and grid compliance. Currently zero test coverage exists for these
rules. This is the most critical testability gap.

**Independent Test**: Can be fully tested by running `cargo test` for the planning service unit
tests. Delivers verified acceptance gate behavior with no infrastructure dependency.

**Acceptance Scenarios**:

1. **Given** a planning service with a mock solver returning a plan with higher total cost than the current active plan, **When** `run_planning_cycle` is called with a Periodic trigger, **Then** the plan is not adopted and the current plan is unchanged.
2. **Given** a planning service with a mock solver, **When** `run_planning_cycle` is called with a DeviceDeviation trigger, **Then** the plan is always adopted regardless of cost delta.
3. **Given** a planning service with no current active plan, **When** `run_planning_cycle` is called, **Then** any returned plan is adopted.
4. **Given** a planning service where the active plan's age exceeds the decay window, **When** `run_planning_cycle` is called with a Periodic trigger, **Then** the new plan is adopted unconditionally.

---

### User Story 2 — Developer tests user request lifecycle without HTTP (Priority: P1)

A developer wants to verify that cancelling a user request correctly sets its status to
ABANDONED and clears any linked EV session. Currently this logic is embedded in route handlers
and cannot be exercised without an HTTP call into the full axum stack.

After this feature, the developer creates a `UserRequestService` with an initial active request,
calls `cancel(id)`, and asserts the returned request has ABANDONED status and the associated EV
session is cleared — in a unit test with no HTTP overhead.

**Why this priority**: User request correctness is directly visible to end users and operators.
A bug where cancellation does not clear the EV session would result in the EV remaining under
managed charging when the user expects to have full control. Equal priority to planning because
both govern user-facing correctness.

**Independent Test**: Can be fully tested by running the `UserRequestService` unit tests in
isolation. No axum router, no HTTP server, no simulator needed.

**Acceptance Scenarios**:

1. **Given** an active user request linked to an EV session, **When** `cancel(id)` is called, **Then** the request status becomes ABANDONED and the linked EV session is cleared.
2. **Given** an attempt to create a duplicate user request for an asset that already has an active request, **When** `create` is called, **Then** an error is returned without modifying state.
3. **Given** a user request that does not exist, **When** `cancel(id)` is called, **Then** a not-found error is returned.

---

### User Story 3 — Developer tests VTN polling without network access (Priority: P2)

A developer wants to write a test for the obligation check loop to verify that a due obligation
triggers a report submission. Today `VtnClient` returns raw JSON with no trait interface, so
there is no way to inject a mock VTN — every test that touches obligation checking requires a
live network call.

After this feature, the developer constructs an `ObligationService` with a `MockVtn` that
returns a fixture obligation and records report submissions, then asserts the correct report
was submitted — with no network.

**Why this priority**: The obligation/reporting path is the VEN's compliance interface with the
grid operator. Regressions are high-consequence but currently invisible. Ranked below planning
and user requests because the obligation path is simpler and less frequently changed.

**Independent Test**: Can be fully tested by running `ObligationService` unit tests with
`MockVtn`. Delivers verified report submission behavior with no network dependency.

**Acceptance Scenarios**:

1. **Given** an obligation that is due and a mock VTN that records calls, **When** `check_and_report` is called, **Then** exactly one report is submitted to the VTN.
2. **Given** no obligations are due, **When** `check_and_report` is called, **Then** no report is submitted.
3. **Given** the VTN returns an error on the first report attempt, **When** `check_and_report` is called, **Then** the error is returned to the caller with no retry — the obligation task loop is responsible for retrying on the next scheduled tick.

---

### User Story 4 — Developer adds a new HEMS command without touching route logic (Priority: P2)

A developer needs to add a new EV charging command. Currently this requires editing a route
handler that mixes HTTP parsing, validation, state mutation, and business rules — making it hard
to reason about and risky to change.

After this feature, the developer adds one method to `EvSessionService`, writes a unit test for
that method, and updates the route handler to call it — the route handler becomes a thin
adapter with no business logic.

**Why this priority**: This delivers ongoing maintainability benefit — every future HEMS feature
becomes cheaper to add and test. Ranked P2 because the immediate risk reduction is lower than
planning and user requests.

**Independent Test**: Can be fully tested by exercising the service method directly. Route
handler test confirms the delegation (no business logic in the handler).

**Acceptance Scenarios**:

1. **Given** an EV session is active, **When** `EvSessionService::end` is called, **Then** the session is cleared and any linked user request transitions to its terminal status.
2. **Given** the departure guard is active for an EV session, **When** the absorber attempts to reduce EV charge, **Then** the reduction is blocked and logged.
3. **Given** a heater target is set, **When** `HvacService::clear_heater_target` is called, **Then** no heater target is present in state.

---

### User Story 5 — VTN responses carry typed fields (Priority: P2)

A developer reading code that processes a VTN event currently sees `.get("programID")` and
`.as_str()` calls scattered throughout the codebase with no indication of which fields are
required, optional, or what their types are. A typo in a field name produces a silent `None`
at runtime rather than a compile error.

After this feature, VTN responses are deserialized into typed structs with named fields. Typos
are caught at compile time. Optional fields are modeled explicitly. The VTN's own field naming
convention is preserved throughout (no translation layer).

**Why this priority**: Type safety prevents an entire class of silent runtime bugs. The work is
bounded and does not affect any business logic — it is a structural upgrade to an existing
interface.

**Independent Test**: Can be fully tested by running deserialisation tests against fixture JSON
strings taken from real VTN responses. Also verified by `cargo check` succeeding with no
`serde_json::Value` in the public VTN interface.

**Acceptance Scenarios**:

1. **Given** a valid VTN program JSON fixture, **When** the typed deserialiser is applied, **Then** all fields are populated correctly with no panic.
2. **Given** a VTN event JSON fixture with an optional field absent, **When** the typed deserialiser is applied, **Then** the optional field is `None` and deserialization succeeds.
3. **Given** the VTN returns a 401 on the first call, **When** any VTN method is invoked, **Then** the token is refreshed automatically and the call succeeds on retry.

---

### User Story 6 — tick.rs stays within the declared module size limit (Priority: P3)

A developer reviewing the `tasks/sim_tick/tick.rs` file observes it exceeds the project's
200-line module limit. The project's own maintainability rules exist to prevent any single
module from becoming a coordination bottleneck, and `tick.rs` is already over the limit.

After this feature, the absorber-params construction is extracted into the existing helpers
module, and `tick.rs` is back under 200 lines.

**Why this priority**: This is a housekeeping fix. The violation exists, but the file is well
structured and the risk of leaving it is low compared to the service layer and VTN typing gaps.
Ranked P3 because it can be done quickly and ships before the other gaps are complete.

**Independent Test**: Verified by counting lines after the extraction. All existing simulator
tick tests continue to pass.

**Acceptance Scenarios**:

1. **Given** the tick.rs file currently at 208 lines, **When** the absorber-params construction is moved to helpers, **Then** tick.rs is under 200 lines and all existing tests pass.
2. **Given** the extracted helper function, **When** the profile values change, **Then** the tick loop picks up the new values correctly on the next call.

---

### Edge Cases

- What happens when a plan cycle is triggered while a previous solve is still running? The service must not start a second parallel solve — the latch/deviation-pending flag must be checked before any solve begins.
- What happens when the VTN returns a field in a JSON response that is not in the typed struct? Unknown fields must be ignored (not cause a deserialisation error) to tolerate VTN version skew.
- What happens when `cancel` is called on a user request that is already in a terminal state (COMPLETED, ABANDONED)? The service must return an error — double-cancel must not change state.
- What happens if the typed VTN struct is missing a field that was previously accessed via `.get()` by a consumer? The consumer must not compile until it switches to the typed field accessor.

---

## Requirements *(mandatory)*

### Functional Requirements

**Services layer (Gap 1)**

- **FR-001**: The system MUST provide a planning service that encapsulates plan triggering, solving, acceptance gate evaluation, and plan adoption as a single callable unit.
- **FR-002**: The planning service MUST accept trigger type as input and apply different acceptance rules for periodic versus event-driven triggers.
- **FR-003**: The planning service MUST support injecting a mock solver and mock simulator so that all acceptance-gate logic can be exercised in unit tests without a live solver process or running simulator.
- **FR-004**: The system MUST provide a user request service that handles create, cancel, and query operations for user requests.
- **FR-005**: The user request service MUST enforce that cancelling a request clears any linked EV session and marks the request ABANDONED in a single atomic operation.
- **FR-006**: The system MUST provide an EV session service and an HVAC service that own all business rules for session lifecycle and heater target management.
- **FR-007**: The system MUST provide an obligation service that encapsulates due-obligation detection and report submission. The service MUST NOT perform retries internally — it returns errors to the caller and the task loop retries on the next scheduled tick.
- **FR-008**: Route handlers MUST NOT contain business logic — each handler MUST delegate to exactly one service call and perform only HTTP serialization/deserialization.
- **FR-009**: Each service MUST have a unit test suite covering all primary scenarios defined in the user stories above, using only mock ports (no live simulator, no network, no solver process).

**Typed VTN interface (Gap 2)**

- **FR-010**: The VTN communication layer MUST expose its public interface using typed domain structs (program, event, report) rather than untyped JSON values. Structs MUST model only the fields currently consumed by the VEN — no speculative fields.
- **FR-011**: The typed structs MUST preserve the VTN's current field naming convention exactly — no renaming or normalization. Fields are taken from existing access patterns in the codebase, not inferred from the OpenADR 3 specification document, to minimize coupling ahead of the OpenADR 3.1 breaking changes.
- **FR-012**: The VTN communication layer MUST be accessible via a trait interface so that a mock implementation can be substituted in tests.
- **FR-013**: A mock VTN implementation MUST be available in the shared test support module for use by obligation service tests and any future service that communicates with the VTN.
- **FR-014**: All consumers of VTN data MUST be updated to use typed field access after the interface change.
- **FR-015**: The VTN adapter contract MUST be verified by tests that deserialize fixture JSON strings — no live network calls in any contract test.
- **FR-016**: Unknown fields returned by the VTN MUST be silently ignored during deserialization.

**tick.rs module size (Gap 3)**

- **FR-017**: The simulator tick module MUST be reduced to under 200 lines by extracting the absorber-params construction into the existing helpers module.
- **FR-018**: The extraction MUST NOT change the behavior of the tick loop — all existing tick tests MUST pass unchanged.

### Key Entities

- **PlanningService**: Encapsulates one full plan cycle (trigger → solve → acceptance gate → adopt/reject). Stateless between cycles — all input comes from injected ports and state snapshots.
- **UserRequestService**: Manages the lifecycle of user energy requests (create, cancel, query). Owns the business rules for linking requests to device sessions.
- **EvSessionService**: Manages the active EV charging session lifecycle. Owns the departure guard rule.
- **HvacService**: Manages heater target state and any HVAC-related business rules.
- **ObligationService**: Detects due obligations and submits the required reports to the VTN. Stateless — reads obligations from state and calls the VTN port.
- **VtnPort**: The communication contract between the VEN and the VTN. Defines fetch and submit operations using typed domain structs.
- **OadrProgram / OadrEvent / OadrReport**: Typed representations of the three primary entity types returned by the VTN. Fields are limited to those currently consumed by the VEN — derived from existing codebase access patterns, not the OpenADR 3 specification document. This minimizes coupling ahead of OpenADR 3.1 breaking field/type name changes.

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: All planning acceptance-gate scenarios (reject below threshold, bypass on hard trigger, bypass after decay window) are covered by unit tests that complete in under 1 second with no external dependencies.
- **SC-002**: All user request lifecycle scenarios (create, cancel, duplicate rejection) are covered by unit tests that complete in under 1 second with no external dependencies.
- **SC-003**: The VTN interface produces zero `serde_json::Value` references in its public method signatures — verified by static analysis.
- **SC-004**: The obligation service can be exercised with a mock VTN in unit tests — no live network call is required to test report submission.
- **SC-005**: The `tasks/sim_tick/tick.rs` file is under 200 lines — verified by line count.
- **SC-006**: All existing tests (cargo test suite) continue to pass after each gap is closed — no regressions introduced.
- **SC-007**: Each route handler in the HEMS route group contains no business logic — verified by code review that each handler body is at most: parse → call one service method → map to HTTP response.
- **SC-008**: All project-declared structural invariants pass after completion: no `use crate::profile` in entities/controller/routes, no untyped JSON in VTN public interface, no tasks/ file exceeds 200 lines.

---

## Assumptions

- No behavior changes are included — this feature is a pure structural refactoring. All existing business logic is moved, not modified.
- The implementation order follows the declared dependency: Gap 3 (tick.rs, standalone) → Gap 2 (VTN typing, standalone) → Gap 1 (services, depends on VtnPort from Gap 2).
- The `services/test_support/` module already contains `MockSimulatorPort` and MILP fixtures — these are reused as-is; only `MockVtn` needs to be added.
- `AppState` already has the correct sub-struct decomposition (`HemsState`, `ControllerSimState`, `PollingState`, `EvSettings`) that maps directly onto the four service boundaries — no state restructuring is needed.
- OpenADR 3 field names for programs, events, and reports are taken from the existing codebase's `.get("fieldName")` access patterns — these represent the actual VTN API surface in use. The OpenADR 3.1 specification introduces breaking changes to type and field names; intentionally minimizing the struct surface now reduces migration cost when 3.1 is adopted later.
