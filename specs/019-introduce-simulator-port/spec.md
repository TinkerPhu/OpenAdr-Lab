# Feature Specification: Introduce SimulatorPort trait (Phase 2 — AB-03)

**Feature Branch**: `019-introduce-simulator-port`  
**Created**: 2026-05-09  
**Status**: Draft  
**Input**: User description: "### Phase 2 — Introduce `SimulatorPort` trait (AB-03)

Define a trait in `controller/`:

```rust
pub trait SimulatorPort: Send + Sync {
    fn snapshot(&self) -> Result<SimSnapshot, SnapshotError>;
    fn inject(&self, state: SimInjectState);
}
```

`SimState` in `simulator/mod.rs` implements `SimulatorPort`. All modules that currently import `S_MOD` directly must switch to `&dyn SimulatorPort`.

Note: `AssetHistoryBuffer` has moved from `simulator/mod.rs` to `assets/mod.rs` — the `SimSnapshot` returned through this port does not carry history; history is a read-only query concern handled separately by the route layer via `assets/mod.rs`. The port trait stays clean.

Estimated effort: **2–3 days"

## Prerequisites

Phase 1 (splitting `loops.rs` into `tasks/`) should be complete before implementing Phase 2. The plan explicitly states that `sim_tick.rs` calling `absorber::apply(...)` and `controller::escalate_if_needed(...)` as named function calls is the AB-03 prerequisite. Implementing Phase 2 inside the monolithic `loops.rs` is significantly harder.

## Clarifications

### Session 2026-05-09

- Q: Should `SimulatorPort::snapshot` return `Result<SimSnapshot, SnapshotError>`, `SimSnapshot` with Option fields, or `Option<SimSnapshot>`? → A: Return `Result<SimSnapshot, SnapshotError>`. **Note**: The architecture plan shows a plain `SimSnapshot` return — this is a deliberate upgrade to allow callers to distinguish uninitialized / transient / fatal failures without panicking.

- Q: Should `SimulatorPort::inject` return `Result<(), InjectError>` or be fire-and-forget (`()`)? → A: Fire-and-forget (`()`). Rationale: injection is a best-effort override (sim override UI); the caller cannot meaningfully recover from a failed inject and the simulator will self-correct on the next tick. If the simulator is uninitialized, the inject is silently dropped.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Unit-test planning and dispatch (Priority: P1)

As a developer, be able to unit-test planning and dispatch logic (e.g., `dispatcher::build_setpoints`, `dispatcher::apply_surplus_ev_overlay`, `dispatcher::apply_battery_correction_overlay`, `absorber::apply_deviation_absorption`, `monitor::record_tick`, `envelope::compute_envelope`) using a mock `SimulatorPort` so tests are fast, deterministic, and do not require the full simulator.

**Why this priority**: These components contain complex business rules currently covered mainly by slow integration tests; making them unit-testable substantially reduces developer turnaround time and test flakiness.

**Independent Test**: Implement a `MockSimulatorPort` that returns deterministic `SimSnapshot` values and records injected states. Unit tests assert function outputs for known inputs.

**Acceptance Scenarios**:

1. **Given** a deterministic mock snapshot, **When** `dispatcher::build_setpoints` is invoked, **Then** returned setpoints match expected values within defined tolerances.

2. **Given** a mock simulator representing a surplus-EV scenario, **When** `dispatcher::apply_surplus_ev_overlay` is invoked, **Then** produced adjustments match expected deltas.

3. **Given** edge deviation values, **When** `absorber::apply_deviation_absorption` runs, **Then** deviations are absorbed according to policy and no panic occurs.

---

### User Story 2 - Small integration validation (Priority: P2)

As a tester, be able to run a small end-to-end tick with the real `SimState` wired behind the new port interface to validate that unit-tested components integrate correctly.

**Independent Test**: A single-tick harness that runs a minimal real simulator and asserts state transitions produced by the controller.

**Acceptance Scenarios**:

1. **Given** a warmed `SimState`, **When** a tick executes, **Then** `monitor::record_tick` records expected metrics and no regressions are observed.

### Edge Cases

- Concurrent access: Multiple asynchronous tasks calling the port concurrently must not cause data races or deadlocks.
- Empty or partial snapshots: Functions must handle absent or partial snapshot fields gracefully (return an explicit error or a safe fallback) rather than panicking.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Define a `SimulatorPort` trait in `controller/` with methods:
  - `snapshot() -> Result<SimSnapshot, SnapshotError>`
  - `inject(state: SimInjectState)`

- **FR-002**: Ensure `SimState` in `simulator/mod.rs` implements `SimulatorPort`.

- **FR-003**: Replace direct `S_MOD` imports in the following modules with injected `&dyn SimulatorPort` parameters:
  - `controller/dispatcher.rs`
  - `controller/absorber.rs`
  - `controller/milp_planner.rs`
  - `controller/monitor.rs`
  - `controller/envelope.rs`
  - `routes/sim.rs` *(temporary — will move to a service in Phase 5)*
  - `routes/timeline.rs` *(temporary — will move to a service in Phase 5)*

- **FR-004**: Move `AssetHistoryBuffer` from `simulator/mod.rs` to `assets/mod.rs`. Ensure the `SimSnapshot` returned by the port does not include history; history remains a read-only query concern handled by assets routes.

- **FR-005**: Add unit tests for `dispatcher::build_setpoints`, `dispatcher::apply_surplus_ev_overlay`, `dispatcher::apply_battery_correction_overlay`, `absorber::apply_deviation_absorption`, `monitor::record_tick`, and `envelope::compute_envelope` using a shared `MockSimulatorPort`.

- **FR-006**: Provide a shared mock adapter (e.g., `services/test_support` or similar) that implements `SimulatorPort` for reuse across tests.

### Key Entities *(include if feature involves data)*

- **SimulatorPort**: Trait providing `snapshot()` and `inject()` operations used by controller modules.
- **SimSnapshot**: Lightweight snapshot of simulation state (without history) returned by `snapshot()`.
- **SimInjectState**: Structure representing state injected into the simulator via `inject()`.
- **AssetHistoryBuffer**: Persisted history buffer moved to `assets/mod.rs`, used by read-only routes.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: All listed modules compile and unit tests for the functions named in FR-005 pass when executed using the `MockSimulatorPort`.

- **SC-002**: At least one unit test exists for each function listed in FR-005, and these tests run in under 30 seconds locally/CI.

- **SC-003**: Existing integration tests that exercise controller behaviour continue to pass after this refactor (no regressions introduced by the port interface change).

- **SC-004**: Code review / automated search confirms that the listed modules no longer import `S_MOD` directly and access simulator functionality exclusively via the `SimulatorPort` interface.
