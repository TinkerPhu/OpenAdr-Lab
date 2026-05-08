# tasks.md

Phase 1: Setup

- [ ] T001 [FR-008, SC-001] Initialize feature branch workspace and record baselines
    - Action: Ensure branch `018-split-loops-tasks` is checked out, working tree clean. Record `cargo test` passing count and BDD baseline. File paths: repo root.

Phase 2: Foundational (blocking prerequisites)

- [ ] T002 [P] [FR-004, FR-001] Create `VEN/src/tasks/mod.rs` with module skeleton and re-export placeholders
    - Action: Add `mod poll_events; mod poll_programs; mod poll_reports; mod obligation; mod planning; mod sim_tick; mod state_persist; mod shared;` and `pub use` placeholders for spawn_* names. File: VEN/src/tasks/mod.rs

- [ ] T003 [P] [FR-003a] Add `VEN/src/tasks/shared.rs` for shared helpers (pub(crate))
    - Action: Create file with module doc and placeholder helper functions. File: VEN/src/tasks/shared.rs

- [X] T013 [P] [FR-006] Add file-size audit job and CI check to enforce 200-line production-code limit
    - Action: Add a CI job (e.g., .github/workflows/file_size_audit.yml) that runs a script measuring production code lines under VEN/src/tasks/ and fails if any file >200 lines excluding #[cfg(test)]. Add local script `scripts/check_task_file_sizes.sh`. File: .github/workflows/file_size_audit.yml

- [X] T014 [FR-001, FR-007] Document locking-preservation and add verification checklist
    - Action: Create `specs/018-split-loops-tasks/locks.md` documenting locks/mutexes affected by migration and add a verification checklist entry in `specs/018-split-loops-tasks/checklists/refactor.md` referencing CHK005. File: specs/018-split-loops-tasks/locks.md

- [X] T015 [P] [Constitution III] Add pre-PR CI checks: cargo fmt, cargo clippy, cargo audit, and enforce DCO
    - Action: Update CI scripts/.github workflows to run cargo fmt -- --check, cargo clippy --all-targets --all-features, cargo audit, and a DCO validation step. File: .github/workflows/ci.yml

User Story Phases (P1 first)

- [X] T004 [US1] [FR-002, FR-007, FR-001] Move event polling (spawn_event_poll) into `VEN/src/tasks/poll_events.rs`
    - Status: verified (cargo test passed for VEN unit tests)
    - Action: Extract spawn_event_poll and exclusive helpers from `VEN/src/loops.rs` into `VEN/src/tasks/poll_events.rs`. Move corresponding `#[cfg(test)]` module into the file. Update `tasks/mod.rs` re-exports. Run `cargo test` and a small BDD subset. If tests fail, revert and debug.

- [X] T005 [US1] [FR-002, FR-007] Move program polling (spawn_program_poll) into `VEN/src/tasks/poll_programs.rs`
    - Status: verified (cargo test passed for VEN unit tests)
    - Action: Extract spawn_program_poll and exclusive helpers into `VEN/src/tasks/poll_programs.rs`. Move tests. Update `tasks/mod.rs`. Run `cargo test` and BDD subset.

- [X] T006 [US1] [FR-002, FR-007] Move report polling (spawn_report_poll) into `VEN/src/tasks/poll_reports.rs`
    - Status: verified (cargo test passed for VEN unit tests)
    - Action: Extract spawn_report_poll and exclusive helpers into `VEN/src/tasks/poll_reports.rs`. Move tests. Update `tasks/mod.rs`. Run `cargo test` and BDD subset.

- [ ] T007 [US2] [FR-002, FR-007] Move obligation check (spawn_obligation_check) into `VEN/src/tasks/obligation.rs`
    - Action: Extract spawn_obligation_check and exclusive helpers into `VEN/src/tasks/obligation.rs`. Move tests. Update `tasks/mod.rs`. Run `cargo test` and BDD subset.

- [ ] T008 [US3] [FR-002, FR-007] Move planning (spawn_planning) into `VEN/src/tasks/planning.rs`
    - Action: Extract spawn_planning and exclusive helpers, ensure solver invocation and context references preserved. Move tests. Update `tasks/mod.rs`. Run `cargo test` and BDD subset.

- [ ] T009 [US3] [FR-002, FR-005, FR-007, FR-006] Move sim tick (spawn_sim_tick) into `VEN/src/tasks/sim_tick.rs` or `tasks/sim_tick/mod.rs` if exceeds 200 lines
    - Action: Extract spawn_sim_tick and its 8 phases into `VEN/src/tasks/sim_tick.rs`, convert absorber correction and deviation escalation into named functions. If production code >200 lines, create `tasks/sim_tick/mod.rs` and move helpers into `tasks/sim_tick/`. Move tests. Update `tasks/mod.rs`. Run `cargo test` and BDD subset.

- [ ] T010 [US3] [FR-002, FR-007] Move state persistence (spawn_state_persist) into `VEN/src/tasks/state_persist.rs`
    - Action: Extract spawn_state_persist and exclusive helpers. Move tests. Update `tasks/mod.rs`. Run `cargo test` and BDD subset.

Phase 4: Finalization

- [ ] T011 [FR-001, FR-004] Delete `VEN/src/loops.rs` and ensure no references to `mod loops` or `crate::loops` remain
    - Action: After all moves and full verification, remove old file, update main.rs `use` statements to `tasks::`. Run full `cargo test` and all BDD scenarios on Pi4-Server.

- [ ] T012 [P] [SC-005, FR-004] Update CI/local scripts and documentation (quickstart.md, plan.md) with new tasks layout
    - Action: Update any scripts referencing `loops.rs` and add note in docs. Files: docs/ and tests/ where applicable.

Parallel opportunities

- T002 and T003 can be created in parallel.
- Moving poll_events/poll_programs/poll_reports can be worked on in parallel by different engineers but must not delete `loops.rs` until all are verified.

Total tasks: 15

Task counts by story

- US1: 3 tasks (T004,T005,T006)
- US2: 1 task (T007)
- US3: 3 tasks (T008,T009,T010)
- Setup/Foundational: 6 tasks (T001,T002,T003,T013,T014,T015)
- Finalization: 2 tasks (T011,T012)

Independent test criteria

- After each moved file, `cargo test` must pass at least the unit tests that reference the moved code and the global passing test count for migrated units must not decrease.
- At finalization, full `cargo test` and BDD suite (232 scenarios) must pass.

MVP suggestion

- Implement User Story 1 (T004-T006) first; this delivers the primary navigability benefit and migrates the most visible code.

Generated tasks.md at: C:\DriveD\Tinker\OpenAdr-Lab\specs\018-split-loops-tasks\tasks.md
