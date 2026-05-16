# Tasks: Fix Architecture Invariant Gaps and Missing Tests

**Input**: Design documents from `/specs/029-fix-arch-invariants-tests/`  
**Prerequisites**: plan.md ✅, spec.md ✅, research.md ✅, data-model.md ✅

**Organization**: Tasks are grouped by user story to enable independent implementation and testing.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1–US4)

---

## Phase 1: Foundational — Doc Fix (US4, zero-risk, run first)

**Purpose**: Fix the stale invariant grep path before any code changes so verification commands are accurate throughout implementation.

- [x] T001 Fix stale `controller/milp` → `controller/milp_planner` in invariant grep command in `docs/plans/ven_backend_architecture_refactoring.md` §8; update surrounding text to clarify `*Params` imports are permitted, only concrete asset types (`A_BAT`, `A_EV`, `A_HTR`) are prohibited
- [x] T002 [P] Fix same stale `controller/milp` → `controller/milp_planner` path in the `ven-architecture` invariant line of `.claude/CLAUDE.md`

**Checkpoint**: Both grep commands now reference the correct directory; running them locally returns empty.

---

## Phase 2: User Story 1 — Remove SimState from ObligationService (P1)

**Goal**: `grep -r "use crate::assets|use crate::simulator" VEN/src/services` returns empty. The obligation service operates only on domain types.

**Independent Test**: `wsl bash -c "grep -r 'use crate::assets\|use crate::simulator' VEN/src/services"` returns no matches. All three existing unit tests in `services/obligation.rs` still pass.

### Implementation for User Story 1

- [x] T003 [US1] In `VEN/src/services/obligation.rs`: change `check_and_report` signature — replace `sim: &Arc<Mutex<SimState>>` with `asset_samples: std::collections::HashMap<String, Vec<crate::controller::reporter::AssetReportSample>>`; remove `use crate::simulator::SimState` (line 8) and `use tokio::sync::Mutex` (verify no other usage of `Mutex` remains in the file after the signature change, then remove unconditionally)
- [x] T004 [US1] In `VEN/src/services/obligation.rs`: remove the internal lock block (current lines 28–49 that lock sim and build asset_samples); replace with direct iteration over the `asset_samples` parameter — the body otherwise stays identical
- [x] T005 [US1] In `VEN/src/services/obligation.rs` test module: remove `use crate::simulator::SimState` (line 89) and delete the `make_sim()` helper entirely (it will have no remaining callers); update `test_check_skips_when_none_due` and `test_check_propagates_vtn_error` to pass `std::collections::HashMap::new()` as the `asset_samples` argument; `test_mock_vtn_error_propagated_when_upsert_called` calls `vtn.upsert_report` directly (not `check_and_report`) and requires no call-site change — confirm this at review
- [x] T006 [US1] In `VEN/src/tasks/obligation.rs`: add imports `use chrono::Duration`, `use crate::controller::reporter::AssetReportSample`, `use std::collections::HashMap`; before the `ObligationService::check_and_report` call, add the lock+extract block that builds `HashMap<String, Vec<AssetReportSample>>` from `sim_guard.assets` using `entry.history.slice(Duration::seconds(3600), now)` and maps `(p.ts, p.power_kw, p.state.soc())`; pass `asset_samples` to the service
- [x] T007 [US1] Run `wsl cargo check --manifest-path VEN/Cargo.toml` and fix any compile errors; then run `wsl bash -c "cd VEN && cargo test services::obligation::tests 2>&1"` to confirm all three obligation service tests pass

**Checkpoint**: Invariant 5 grep returns empty. Obligation service tests pass. `cargo check` clean.

---

## Phase 3: User Story 2 — tick_once Smoke Test (P2)

**Goal**: `tick_once_runs_without_profile` unit test exists and passes in `cargo test`.

**Independent Test**: `wsl bash -c "cd VEN && cargo test tick_tests 2>&1 | tail -10"` shows 1 test passed.

### Implementation for User Story 2

- [x] T008 [US2] In `VEN/src/controller/absorber.rs`: add `Default` to the derive macro on `AbsorberState` — change `#[derive(Debug, Clone)]` to `#[derive(Debug, Clone, Default)]`
- [x] T009 [US2] In `VEN/src/tasks/sim_tick/tick.rs`: append `#[cfg(test)] mod tick_tests;` at the end of the file (after the closing `}` of `tick_once`); confirm resulting line count is ≤ 200 with `wsl bash -c "wc -l VEN/src/tasks/sim_tick/tick.rs"` (exact count depends on blank-line separator; both 194 and 195 are within limit)
- [x] T010 [US2] Create new file `VEN/src/tasks/sim_tick/tick_tests.rs` with a `#[cfg(test)] mod tests` block containing the `tick_once_runs_without_profile` test; import `tick_once` with `use crate::tasks::sim_tick::tick::tick_once` (NOT `super::tick_once` — `tick_tests.rs` is a sibling module of `tick` under `sim_tick`, not a child of `tick`); the test builds a minimal `SimState` via `serde_json::from_value`, constructs `AbsorberState::default()`, `AbsorberParams::default()`, `AppState::new()`, `Arc<dyn VtnPort>` wrapping `MockVtn::new()`, `Arc::new(watch::channel(PlanTrigger::Periodic).0)`, `Arc::new(broadcast::channel::<PlannerEvent>(1).0)`, and `Arc::new(AtomicBool::new(false))`; calls `tick_once` with counters `persist_counter=0, persist_every_ticks=100, report_counter=0, report_every_ticks=100, tick_s=1`; asserts no panic (test passes on completion)
- [x] T011 [US2] Run `wsl cargo check --manifest-path VEN/Cargo.toml` to verify compile; fix any import or type errors in `tick_tests.rs`; then run `wsl bash -c "cd VEN && cargo test tick_tests 2>&1 | tail -15"` to confirm the test passes

**Checkpoint**: `tick_once_runs_without_profile` passes. `tick.rs` ≤ 200 lines. `cargo check` clean.

---

## Phase 4: User Story 3 — spawn_planning Smoke Test (P3)

**Goal**: `spawn_planning_constructs_without_panic` unit test exists and passes in `cargo test`.

**Independent Test**: `wsl bash -c "cd VEN && cargo test planning::tests 2>&1 | tail -10"` shows 1 test passed.

### Implementation for User Story 3

- [x] T012 [US3] In `VEN/src/tasks/planning.rs`: append a `#[cfg(test)] mod tests` block at the end of the file (after line 258) containing `spawn_planning_constructs_without_panic`; the test module needs its own `use crate::simulator::SimState` import for the `minimal_sim()` helper (the module-level import at line 12 is not in scope inside the test block); constructs a minimal `SimState` via `serde_json::from_value`, creates `watch::channel(PlanTrigger::Periodic)`, `broadcast::channel::<PlannerEvent>(1)`, `Arc::new(MockVtn::new())`, `Arc::new(RwLock::new(PlannerObjective::default()))`, `Arc::new(AtomicBool::new(false))`; calls `spawn_planning(AppState::new(), PlannerParams::default(), 10.0, 10.0, vec![], vtn, "test-ven".to_string(), trigger_rx, sim, active_objective, event_tx, deviation_pending)`; immediately calls `.abort()` on the returned handle; test passes if no panic
- [x] T013 [US3] Run `wsl cargo check --manifest-path VEN/Cargo.toml` to verify compile; fix any import errors; then run `wsl bash -c "cd VEN && cargo test planning::tests 2>&1 | tail -15"` to confirm the test passes; also confirm `planning.rs` stays ≤ 500 lines with `wsl bash -c "wc -l VEN/src/tasks/planning.rs"`

**Checkpoint**: `spawn_planning_constructs_without_panic` passes. `planning.rs` ≤ 500 lines. `cargo check` clean.

---

## Phase 5: Polish & Full Verification

**Purpose**: Run all invariant checks, full unit test suite, and BDD gate.

- [x] T014 Run all five architecture invariant greps and confirm each returns empty output (greps 1–4 should have been empty before this branch; grep 5 should now be empty after US1 — any match is a regression):
  - `wsl bash -c "grep 'use crate::simulator\|use crate::assets' VEN/src/controller/reporter.rs"`
  - `wsl bash -c "grep 'use crate::assets' VEN/src/controller/timeline.rs"`
  - `wsl bash -c "grep -r 'use crate::profile' VEN/src/tasks"`
  - `wsl bash -c "grep -r 'use crate::vtn::VtnClient' VEN/src/tasks"`
  - `wsl bash -c "grep -r 'use crate::assets\|use crate::simulator' VEN/src/services"`
- [x] T015 [P] Run full unit test suite: `wsl bash -c "cd VEN && cargo test 2>&1 | tail -30"` — confirm zero failures including the two new tests (`tick_once_runs_without_profile`, `spawn_planning_constructs_without_panic`); this fulfils the Phase 3 and Phase 4 test deliverables mandated by `docs/plans/ven_backend_architecture_refactoring.md` §6
- [ ] T016 Push branch to remote and deploy to Pi4-Server: `git push origin 029-fix-arch-invariants-tests` then `ssh Pi4-Server "cd /srv/docker/openadr_lab && git pull && git checkout 029-fix-arch-invariants-tests"` then `ssh Pi4-Server "cd /srv/docker/openadr_lab && docker compose run --rm ven-test 2>&1 | tail -30"` — confirm 44 features, 238 scenarios, 0 failures
- [x] T017 [P] Update `docs/history/project_journal.md` with what was done, why, and any key learnings from this feature

---

## Dependencies & Execution Order

### Phase Dependencies

- **Foundational (Phase 1)**: No dependencies — start immediately; completes in minutes
- **US1 (Phase 2)**: No code dependencies on Phase 1; can start after Phase 1 or concurrently
- **US2 (Phase 3)**: No dependencies on US1 — can start immediately after Phase 1
- **US3 (Phase 4)**: No dependencies on US1 or US2 — can start immediately after Phase 1
- **Polish (Phase 5)**: Depends on all phases complete

### User Story Dependencies

- **US1 (P1)**: Independent — changes `services/obligation.rs` and `tasks/obligation.rs`
- **US2 (P2)**: Independent — changes `controller/absorber.rs`, `tasks/sim_tick/tick.rs`, creates `tick_tests.rs`
- **US3 (P3)**: Independent — changes `tasks/planning.rs` only
- **US4 (P4)**: Foundational — pure doc fix, no source changes

### Parallel Opportunities

- T001 and T002 touch different files — can run in parallel
- T003, T004, T005, T006 all touch `services/obligation.rs` — must run sequentially within US1
- T008, T009, T010 touch different files — can run in parallel within US2
- T014 and T015 (Polish checks) can run in parallel
- T015 and T017 can run in parallel (different outputs)
- US2 (T008–T011) and US3 (T012–T013) touch completely different files — can be worked in parallel

---

## Parallel Example: US2 + US3

```
# US2 and US3 can be implemented simultaneously (different files):

US2 Thread:
  T008 absorber.rs → Default derive
  T009 tick.rs → add mod declaration
  T010 tick_tests.rs → create test file
  T011 cargo check + test

US3 Thread (simultaneously):
  T012 planning.rs → add test module
  T013 cargo check + test
```

---

## Implementation Strategy

### MVP First (US1 — the invariant violation)

1. Complete Phase 1 (doc fix) — 5 minutes
2. Complete Phase 2 (US1, SimState removal) — invariant is now clean
3. **STOP and VALIDATE**: run invariant grep + unit tests
4. Continue with US2, US3 in parallel

### Incremental Delivery

1. Phase 1 → invariant verification commands are correct
2. US1 (Phase 2) → Invariant 5 clean, obligation tests passing
3. US2 (Phase 3) → tick_once has coverage
4. US3 (Phase 4) → spawn_planning has smoke test
5. Polish (Phase 5) → BDD gate confirms no regressions

---

## Notes

- Run `wsl cargo check` after each user story to catch compile errors early
- `tick_tests.rs` is declared with `mod tick_tests;` inside `tick.rs`, making it a sibling of `tick` under the `sim_tick` module — use `use crate::tasks::sim_tick::tick::tick_once` (NOT `super::tick_once`, which would resolve to `sim_tick::tick_once` and fail)
- After T003–T005, the two obligation service tests that call `check_and_report` must pass `HashMap::new()` as `asset_samples`; the third test (`test_mock_vtn_error_propagated_when_upsert_called`) calls `vtn.upsert_report` directly and needs no update
- `AbsorberState::Default` sets all `HashMap` fields to empty and numeric fields to zero — verified safe from struct definition
- The `spawn_planning` test aborts before the 5-second startup sleep completes; no MILP solver is invoked
- After BDD gate passes, this branch is ready to merge to main
