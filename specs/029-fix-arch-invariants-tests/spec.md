# Feature Specification: Fix Architecture Invariant Gaps and Missing Tests

**Feature Branch**: `029-fix-arch-invariants-tests`  
**Created**: 2026-05-16  
**Status**: Draft  
**Input**: User description: "all items of docs\plans\post_refactoring_fixes.md except item 1 which is already implemented and currently executed and tested."

## Context

Post-implementation verification of the VEN backend architecture refactoring (phases 1–4) revealed four remaining gaps. Item 1 (VtnClient in task files) is already resolved. This feature addresses Items 2–5:

- **Item 2**: `services/obligation.rs` still imports `SimState` from the simulator layer, violating the hexagonal architecture boundary (services must not depend on infra/simulator directly).
- **Item 3**: The `tick_once()` function in the sim-tick task has no unit test, which was a deliverable in Phase 3.
- **Item 4**: `spawn_planning()` has no smoke test, which was a deliverable in Phase 4.
- **Item 5**: A stale directory path in the architecture reference doc causes the invariant grep command to silently return wrong results.

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Obligation Service No Longer Leaks Simulator Dependency (Priority: P1)

A developer running the architecture invariant check on `services/` gets a clean result (empty output). The obligation service receives pre-extracted asset samples from its caller instead of locking the simulator directly, enforcing the hexagonal boundary.

**Why this priority**: This is a hard architectural invariant. Every other service was already cleaned; this is the last violation. Leaving it unresolved makes the invariant check unreliable.

**Independent Test**: Running the invariant grep `grep -r "use crate::assets|use crate::simulator" VEN/src/services` returns empty. Unit tests for `ObligationService` pass without any simulator dependency.

**Acceptance Scenarios**:

1. **Given** the obligation service source file, **When** the invariant grep is executed, **Then** no matches are found for `use crate::simulator` or `use crate::assets` in any file under `services/`.
2. **Given** an obligation check cycle, **When** the task layer provides pre-built asset sample maps, **Then** the service processes them correctly and produces the same reporting output as before.
3. **Given** the obligation service unit tests, **When** they are run without a simulator instance, **Then** all tests pass using only domain types.

---

### User Story 2 — `tick_once` Has a Smoke Test (Priority: P2)

A developer can run the unit test suite and confirm that `tick_once()` executes end-to-end with a minimal set of inputs (no YAML config, no profile loading) without panicking.

**Why this priority**: Phase 3 committed to this test as a deliverable. Missing it leaves a core simulation step untested and breaks CI confidence.

**Independent Test**: Running `cargo test` shows `tick_once_runs_without_profile` passes. The test file is a separate module under `sim_tick/` so it can be reviewed and executed in isolation.

**Acceptance Scenarios**:

1. **Given** default/minimal structs for all `tick_once` parameters, **When** `tick_once()` is awaited in a test, **Then** it completes without panic and returns valid output tuples.
2. **Given** the `tick.rs` file, **When** its line count is checked, **Then** it remains at or below 200 lines after adding the module declaration.

---

### User Story 3 — `spawn_planning` Has a Smoke Test (Priority: P3)

A developer can verify that the planning task can be constructed and started without crashing, using a mock VTN and minimal planner configuration.

**Why this priority**: Phase 4 committed to this test. It validates that wiring of the VTN port abstraction into the planning task is correct end-to-end.

**Independent Test**: Running `cargo test` shows the `spawn_planning` smoke test passes. The test immediately aborts the spawned task handle after construction, verifying no panic on startup.

**Acceptance Scenarios**:

1. **Given** a mock VTN, minimal planner params, empty asset list, and required channels, **When** `spawn_planning(...)` is called and the handle is aborted, **Then** no panic occurs.
2. **Given** the planning task test module, **When** run in isolation, **Then** no real VTN network calls are made (mock intercepts all calls).

---

### User Story 4 — Architecture Doc Invariant Grep Uses Correct Directory (Priority: P4)

A developer following the architecture reference document runs the invariant grep for `use crate::assets::` and targets the correct directory, getting accurate results.

**Why this priority**: A stale path silently returns empty results even if violations exist — the invariant check becomes a false safety net. This is a documentation correctness fix.

**Independent Test**: The grep command in the architecture doc, when copy-pasted into a terminal, targets `controller/milp_planner` (the actual directory). Running it against the codebase returns empty (no violations).

**Acceptance Scenarios**:

1. **Given** the architecture reference doc, **When** the invariant grep command is read, **Then** it references `controller/milp_planner` not `controller/milp`.
2. **Given** the updated doc, **When** the grep is executed, **Then** it returns empty, confirming no concrete asset type imports exist in the MILP planner.

---

### Edge Cases

- What happens if `AbsorberParams` or `AbsorberState` do not derive `Default`? A `Default` impl must be added or a test constructor provided before the test can compile.
- What happens if `SimState` has no test constructor? A `default_for_test()` or `Default` impl must be provided in the simulator module.
- What if the obligation task builds an empty asset sample map (no assets in sim)? The service must handle empty maps gracefully without panicking.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The obligation service MUST NOT import `SimState` or any type from `crate::simulator` or `crate::assets`.
- **FR-002**: The obligation task (caller) MUST extract asset samples from the simulator lock before calling the obligation service, passing domain types only.
- **FR-003**: The `tick_once()` function MUST have at least one unit test that executes it end-to-end with default/minimal parameters.
- **FR-004**: The `tick.rs` file MUST remain at or below 200 lines after adding the test module declaration.
- **FR-005**: The `spawn_planning()` function MUST have at least one smoke test that constructs the task with a mock VTN and aborts it without panic.
- **FR-006**: The architecture reference document MUST use `controller/milp_planner` (not `controller/milp`) in all invariant grep commands referencing that directory.
- **FR-007**: All five architecture invariant greps (from ch7 of the refactoring plan) MUST return empty after these fixes.
- **FR-008**: The full unit test suite MUST pass after all changes.

### Key Entities

- **AssetReportSample**: Domain type carrying timestamp, power reading, and state-of-charge per asset per time-point; used to transfer simulator data across the architecture boundary without leaking simulator types.
- **ObligationService**: Application-layer service responsible for checking obligations and submitting reports to the VTN. Must depend only on domain types and ports.
- **tick_once**: Core sim-tick function; stateless across calls, accepts all dependencies as parameters.
- **spawn_planning**: Async task launcher for the MILP planner loop; wires the VTN port abstraction, planner params, and event channels.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: All five architecture invariant grep commands return empty output (zero violations across tasks, services, controller layers).
- **SC-002**: Running the unit test suite completes with zero test failures, including the two new tests (`tick_once` smoke test and `spawn_planning` smoke test).
- **SC-003**: The `tick.rs` file line count is 200 or fewer after adding the test module declaration.
- **SC-004**: The BDD suite on the integration server passes with the same or better results as the baseline (44 features, 238 scenarios) after all changes are deployed.
- **SC-005**: The architecture reference document's invariant grep section references the correct directory path, and a developer following it gets accurate results.

## Assumptions

- `AssetReportSample` is already exported from `controller/reporter.rs` and usable in the task layer without circular imports.
- The `MockVtn` test helper exists and implements the VTN port trait fully enough to use in task-layer tests.
- The obligation service change is purely a parameter substitution — no business logic changes; the same data flows through with different type wrappers.
- Adding a `Default` impl to `AbsorberParams` and `AbsorberState` (if missing) is safe and has no side effects on existing code.
