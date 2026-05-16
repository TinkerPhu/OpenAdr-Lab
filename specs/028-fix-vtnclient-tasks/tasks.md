# Tasks: Fix VtnClient References in Remaining Task Files

**Input**: Design documents from `/specs/028-fix-vtnclient-tasks/`  
**Prerequisites**: plan.md ✅, spec.md ✅, research.md ✅

**Tests**: No separate test tasks — acceptance is the invariant grep + `cargo check`. Existing BDD suite validates runtime behavior unchanged.

**Organization**: The 4 task-file changes are fully independent (different files) and run in parallel. `main.rs` call-site update follows once all 4 signatures are consistent.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no shared dependencies)
- **[Story]**: Which user story this task belongs to

---

## Phase 1: User Story 1 — Task Files (Priority: P1) 🎯 MVP

**Goal**: Replace `VtnClient` concrete type with `Arc<dyn VtnPort>` in all four task files, remove the concrete import, and remove the intermediate cast variable inside each async loop.

**Independent Test**: `wsl bash -c "grep -r 'use crate::vtn::VtnClient' VEN/src/tasks"` returns empty output and `wsl cargo check --manifest-path VEN/Cargo.toml` succeeds. Covers both US1 and US2 acceptance criteria.

- [x] T001 [P] [US1] In `VEN/src/tasks/poll_programs.rs`: remove `use crate::vtn::VtnClient`, add `use std::sync::Arc`, change param `vtn: VtnClient` → `vtn: Arc<dyn VtnPort>`, remove cast line `let vtn_port: &dyn VtnPort = &vtn;`, replace `vtn_port.fetch_programs()` with `vtn.fetch_programs()`
- [x] T002 [P] [US1] In `VEN/src/tasks/poll_reports.rs`: remove `use crate::vtn::VtnClient`, add `use std::sync::Arc`, change param `vtn: VtnClient` → `vtn: Arc<dyn VtnPort>`, remove cast line `let vtn_port: &dyn VtnPort = &vtn;`, replace `vtn_port.fetch_reports_raw()` with `vtn.fetch_reports_raw()`
- [x] T003 [P] [US1] In `VEN/src/tasks/poll_events.rs`: remove `use crate::vtn::VtnClient` (line 12), change param `vtn: VtnClient` → `vtn: Arc<dyn VtnPort>` in `spawn_event_poll`, remove cast line `let vtn_port: &dyn VtnPort = &vtn;` (line 132), replace `vtn_port.fetch_events()` with `vtn.fetch_events()` (`Arc` and `VtnPort` already imported)
- [x] T004 [P] [US1] In `VEN/src/tasks/obligation.rs`: remove `use crate::vtn::VtnClient`, add `use crate::controller::VtnPort`, change param `vtn: VtnClient` → `vtn: Arc<dyn VtnPort>`, change call argument from `&vtn` to `vtn.as_ref()` in `ObligationService::check_and_report(...)` (`Arc` already imported)
- [x] T005 [US1] In `VEN/src/main.rs`: change the 4 spawn call sites to pass `vtn_port.clone()` instead of `vtn.clone()`: lines 188 (`spawn_program_poll`), 191 (`spawn_event_poll`), 195 (`spawn_report_poll`), 216 (`spawn_obligation_check`) — `vtn` at line 242 (`AppCtx { vtn, ... }`) remains unchanged

**Checkpoint**: After T001–T005, `cargo check` must pass and the invariant grep must return empty. US1 and US2 are both satisfied at this point (casts removed as part of T001–T004).

---

## Phase 2: Polish & Verification

**Purpose**: Confirm all architecture invariants hold and the full test suite passes.

- [x] T006 Run full invariant grep suite: Inv4 PASS, Inv1 PASS, Inv2 PASS, Inv3 PASS; Inv5 fails (pre-existing, services/obligation.rs — Item 2 scope, not this feature)
- [x] T007 Run `wsl cargo check --manifest-path VEN/Cargo.toml` — PASS (Finished dev profile, 0 errors, 42 pre-existing warnings; initial run hit transient rustc ICE in diagnostic renderer, resolved with --message-format=short)
- [ ] T008 Run BDD suite on Pi4-Server: `ssh Pi4-Server "cd /srv/docker/openadr_lab && docker compose run --rm ven-test 2>&1 | tail -30"` — all scenarios must pass
- [x] T009 Update `docs/history/project_journal.md` with what was done, why, and any key learnings from this change

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (T001–T004)**: All four task-file changes are fully independent — run in parallel
- **Phase 1 (T005)**: Depends on T001–T004 being complete (signatures must be updated before call sites)
- **Phase 2 (T006–T009)**: Depends on T001–T005 completion

### Parallel Opportunities

```
# Phase 1 — run simultaneously (different files, no shared state):
T001  VEN/src/tasks/poll_programs.rs
T002  VEN/src/tasks/poll_reports.rs
T003  VEN/src/tasks/poll_events.rs
T004  VEN/src/tasks/obligation.rs

# Then (depends on T001–T004 signature consistency):
T005  VEN/src/main.rs

# Then verification:
T006  invariant greps
T007  cargo check
T008  Pi4-Server BDD suite
T009  journal
```

---

## Implementation Strategy

### MVP (All tasks required — no partial delivery)

This feature is a single atomic invariant fix. All 5 implementation tasks (T001–T005) must be
complete before verification can run. The recommended order:

1. Complete T001–T004 in parallel (or sequentially — any order)
2. Run intermediate `wsl cargo check` after each file to catch errors early
3. Complete T005 (`main.rs` call sites)
4. Run full verification (T006–T008)
5. Update journal (T009)

---

## Notes

- [P] tasks touch different files — safe to implement simultaneously
- `vtn` at `main.rs:242` is intentionally kept as `VtnClient` (used in `AppCtx` for routes layer)
- `poll_events.rs` already imports `use std::sync::Arc` — do not add a duplicate
- `obligation.rs` task has no `VtnPort` import today — must add it (see T004)
- After this change, `use crate::vtn::VtnClient` must appear ONLY in `main.rs` and `vtn.rs` itself
