# Feature Specification: Split loops.rs into tasks/ Module (Phase 1)

**Feature Branch**: `018-split-loops-tasks`  
**Created**: 2026-05-08  
**Status**: Draft  
**Input**: User description: "Split VEN loops.rs god module into tasks/ directory. One file per background task concern. Phase 1 of VEN backend architecture refactoring."

## Clarifications

### Session 2026-05-08

- Q: Should locking/concurrency semantics be changed in this refactor? → A: Preserve existing locking semantics exactly (no concurrency changes)
- Q: Should the 200-line limit include tests and comments? → A: Exclude #[cfg(test)] test modules from the 200-line limit
- Q: Where should helper functions used by multiple spawn_* functions be placed? → A: Place shared helpers in `tasks/shared.rs` (internal to tasks)
- Q: What migration approach should be used to move functions/files from `loops.rs` to `tasks/`? → A: Incremental file-by-file migration with compatibility re-exports

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Navigate to a Concern Without Scanning 1000 Lines (Priority: P1)

A developer who needs to change event polling behaviour, the MILP planning loop, or the simulator tick
currently must open a 1077-line file and scan it to find the right function. After this change, they
open the `tasks/` directory, read the file name (e.g. `poll_events.rs`, `planning.rs`, `sim_tick.rs`),
and go directly to the relevant code.

**Why this priority**: The primary motivation for Phase 1 is navigability. Every subsequent change to
any background task benefits immediately, and it is the prerequisite for all later refactoring phases.

**Independent Test**: Open the `tasks/` directory listing. Every background concern is represented by
a dedicated file. Each file is readable in full without scrolling.

**Acceptance Scenarios**:

1. **Given** the restructure is complete, **When** a developer lists `VEN/src/tasks/`, **Then** they
   see one file per concern: `mod.rs`, `poll_events.rs`, `poll_programs.rs`, `poll_reports.rs`,
   `obligation.rs`, `planning.rs`, `sim_tick.rs`, `state_persist.rs` — and no other `.rs` files.
2. **Given** any single file in `tasks/`, **When** its line count is measured, **Then** it does not
   exceed 200 lines.
3. **Given** `VEN/src/loops.rs` existed before, **When** the restructure is complete, **Then** that
   file no longer exists.

---

### User Story 2 — Add a Test for a Task Concern in the Right Place (Priority: P1)

A developer adding a new test for `detect_event_changes()` or `accumulate_deviation()` knows exactly
which file to edit — the same file that contains the function under test. Tests do not live in a
separate god module; they live beside their subject code.

**Why this priority**: Co-location of tests with the code they cover is the prerequisite for the
domain-test layer described in the architecture refactoring plan. All subsequent testing phases depend
on this structural pattern being established now.

**Independent Test**: All 13 existing tests (5 event poll tests, 8 sim tick tests) are present and
passing in their respective task files immediately after the restructure.

**Acceptance Scenarios**:

1. **Given** `tasks/poll_events.rs` exists, **When** `cargo test` is run, **Then** the 5 tests from
   the original `event_poll_tests` module (`new_event_emits_arrived`, `removed_event_emits_expired`,
   `tariff_count_change_emits_rate_change`, `import_limit_change_emits_capacity_change`,
   `no_changes_emits_nothing`) pass within that file's test module.
2. **Given** `tasks/sim_tick.rs` exists, **When** `cargo test` is run, **Then** the 8 tests from
   the original `sim_tick_tests` module (`test_build_setpoints_no_plan`,
   `finalize_tick_outputs_returns_sensor_and_snap`, `finalize_tick_outputs_pushes_history`,
   `finalize_tick_outputs_updates_grid_asset`, `accumulate_deviation_increments_residual_ticks_when_above_threshold`,
   `accumulate_deviation_fires_devicedeviation_at_threshold`,
   `accumulate_deviation_ignores_residual_within_deadband`,
   `accumulate_deviation_resets_ticks_on_recovery`) pass within that file's test module.
3. **Given** `loops.rs` is removed, **When** `cargo test` is run, **Then** the total passing test
   count is identical to the baseline recorded before the restructure — no tests are lost.

---

### User Story 3 — All Existing Behaviours Are Preserved Identically (Priority: P1)

The VEN backend starts, polls programs/events/reports, runs the MILP planner, ticks the simulator,
checks obligations, and persists state — all exactly as before. No runtime behaviour changes. The
BDD test suite is the gate.

**Why this priority**: This is a pure structural refactoring. Any behaviour change is a regression.

**Independent Test**: The full BDD test suite (232 scenarios) passes after the restructure with zero
failures and the VEN image rebuilt from the new source layout.

**Acceptance Scenarios**:

1. **Given** the restructure is complete and the VEN Docker image is rebuilt, **When** the BDD test
   suite runs on the Pi4 test runner, **Then** all 232 scenarios pass with zero failures.
2. **Given** `VEN/src/main.rs`, **When** the restructure is complete, **Then** `main.rs` contains
   zero logic changes — the only edit is updating the module reference from `loops::` to `tasks::`.
3. **Given** the sim tick's 8 phases, **When** `tasks/sim_tick.rs` is read, **Then** absorber
   correction (Layer 1) and deviation escalation (Layer 2) are each invoked as an explicit named
   function call rather than an anonymous inline block, making each phase independently locatable.

---

### Edge Cases

- `main.rs` must compile without argument or call-order changes — only module path changes.
- All 13 existing tests must migrate with their functions; none may be silently dropped.
- `spawn_report_poll()` is confirmed in `main.rs` call sites but falls between lines 248–280 in
  `loops.rs` (not in the primary function inventory). It must be included in `tasks/poll_reports.rs`.
- If `tasks/sim_tick.rs` exceeds 200 lines after collecting all 8 phase helpers, the helpers move
  to `tasks/sim_tick/` sub-files; the 200-line cap applies per file, not per directory.
- Migration approach: Perform an incremental, file-by-file migration. For each moved file, add
  compatibility re-exports in `tasks/mod.rs`, run `cargo test` and the BDD subset, and record the
  baseline before the first file is moved.

---

## Requirements *(mandatory)*

- NFR-001: Preserve existing runtime and locking semantics exactly; Phase 1 must not change lock granularity, ordering, or timing. Concurrency improvements are deferred to a later phase and tracked separately.

### Functional Requirements

- **FR-001**: `VEN/src/loops.rs` MUST be deleted; every function it contained MUST reside in a file
  under `VEN/src/tasks/`.
- **FR-002**: Each `spawn_*` function MUST reside in exactly one `tasks/` file named after its
  concern (`spawn_planning()` → `tasks/planning.rs`, `spawn_sim_tick()` → `tasks/sim_tick.rs`, etc.).
- **FR-003**: Helper functions called exclusively by one `spawn_*` function MUST reside in the same
  file as that `spawn_*` function.
- **FR-003a**: Helper functions used by multiple `spawn_*` functions MUST be placed in a shared
  module under `tasks/` (e.g., `tasks/shared.rs` or `tasks/utils.rs`); keep visibility `pub(crate)` unless public exposure is explicitly required.
- **FR-004**: `tasks/mod.rs` MUST re-export all `spawn_*` public names so that `main.rs` requires
  only a module path change (`loops::` → `tasks::`) with zero logic edits.
- **FR-005**: In `tasks/sim_tick.rs`, the absorber correction step and the deviation escalation step
  MUST be invoked as explicit named function calls — not anonymous inline blocks — so each phase is
  independently locatable and unit-testable.
- **FR-006**: No file in `tasks/` MUST exceed 200 lines, excluding #[cfg(test)] test modules. If any file would exceed 200 lines (production code), its
  sub-concerns MUST be extracted to a sub-directory.
- **FR-007**: All existing `#[cfg(test)]` test functions from `loops.rs` MUST migrate to the
  `tasks/` file that owns their subject code. No test function may be deleted or renamed.
- **FR-008**: The total count of passing `cargo test` cases MUST be identical before and after the
  restructure (baseline recorded before, verified after).
- **FR-009**: All 232 BDD scenarios MUST pass after the restructure with the rebuilt VEN image.

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: `cargo test` reports the same number of passing tests before and after the restructure.
  Baseline is recorded before the first file is moved; verified after the last file is moved and
  `loops.rs` is deleted.
- **SC-002**: After the restructure, no reference to `crate::loops` or `mod loops` exists anywhere
  in `VEN/src/` — all references use `tasks::` instead.
- **SC-003**: No file in `VEN/src/tasks/` exceeds 200 lines after the restructure.
- **SC-004**: All 232 BDD scenarios pass on the Pi4 test runner after the restructure, with the VEN
  image rebuilt from the new source layout.
- **SC-005**: `VEN/src/main.rs` requires zero changes to its logic, argument order, or startup
  sequence — the only edit is the module path in `use` statements.
