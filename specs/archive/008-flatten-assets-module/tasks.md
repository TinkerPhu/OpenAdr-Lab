# Tasks: Flatten Assets Module

**Input**: Design documents from `/specs/008-flatten-assets-module/`
**Prerequisites**: plan.md ✅ spec.md ✅ research.md ✅ data-model.md ✅

**Organization**: Pure structural refactor — file moves + import path updates.
No new logic. No new tests needed (existing BDD suite provides full coverage).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1 or US2)

---

## Phase 1: Setup

**Purpose**: Confirm the baseline builds cleanly before touching anything.

- [x] T001 Verify `cargo build` passes on branch `008-flatten-assets-module` (confirm clean baseline before any file moves) in `VEN/`

---

## Phase 2: Foundational — Create `VEN/src/assets/` module

**Purpose**: Populate the new top-level `assets/` module with all six moved files, then wire it
into the crate. Must complete before US1 (build verification) or US2 (test verification).

**⚠️ CRITICAL**: No user story work can begin until this phase is complete.

- [x] T002 Create `VEN/src/assets/mod.rs` — copy content verbatim from `VEN/src/simulator/assets/mod.rs`; update the five `pub mod` declarations at the top if they reference a non-relative path (they are relative, so no change expected)
- [x] T003 [P] Create `VEN/src/assets/pv.rs` — copy content verbatim from `VEN/src/simulator/assets/pv.rs`
- [x] T004 [P] Create `VEN/src/assets/battery.rs` — copy content verbatim from `VEN/src/simulator/assets/battery.rs`
- [x] T005 [P] Create `VEN/src/assets/ev.rs` — copy content verbatim from `VEN/src/simulator/assets/ev.rs`
- [x] T006 [P] Create `VEN/src/assets/heater.rs` — copy content verbatim from `VEN/src/simulator/assets/heater.rs`
- [x] T007 [P] Create `VEN/src/assets/base_load.rs` — copy content verbatim from `VEN/src/simulator/assets/base_load.rs`
- [x] T008 Update `VEN/src/simulator/mod.rs` — remove `pub mod assets;`; add re-export bridge (depends on T002–T007):
  ```rust
  pub mod assets {
      pub use crate::assets::*;
  }
  ```
  Also update `use assets::` within the file to `use crate::assets::…`
- [x] T009 Add `pub mod assets;` declaration to `VEN/src/main.rs` alongside the existing `mod simulator;` line (depends on T002)

**Checkpoint**: Foundation ready — new module is declared and wired; ready for build verification.

---

## Phase 3: User Story 1 — Developer navigates to the new asset module (Priority: P1) 🎯 MVP

**Goal**: `VEN/src/assets/` is the canonical home for all asset code; `simulator/assets/` is gone; `cargo build` is clean.

**Independent Test**: `cargo build` exits 0 with zero errors and zero new warnings; `ls VEN/src/assets/` shows six files; `ls VEN/src/simulator/assets/` returns "not found".

### Implementation for User Story 1

- [x] T010 [US1] Run `cargo build` in `VEN/` and fix every compiler error caused by stale `use crate::simulator::assets::…` references in any file (likely `VEN/src/controller/planner.rs`, `VEN/src/controller/dispatcher.rs`, `VEN/src/entities/asset.rs`); update each to `use crate::assets::…` (depends on T008, T009)
- [x] T011 [US1] Delete `VEN/src/simulator/assets/` directory (all six files) — only after T010 produces a clean build (depends on T010)
- [x] T012 [US1] Run `cargo build` again in `VEN/` to confirm zero errors and zero warnings after deletion (depends on T011)
- [x] T013 [US1] Run `grep -r "simulator::assets" VEN/src/` and confirm zero matches (or only re-export lines in `simulator/mod.rs`) (depends on T012)

**Checkpoint**: User Story 1 complete — new directory structure is live, old path is gone, build is clean.

---

## Phase 4: User Story 2 — All existing tests continue to pass (Priority: P2)

**Goal**: `cargo test --workspace` and the full BDD suite pass with no regressions.

**Independent Test**: `cargo test --workspace` exits 0; BDD suite reports 895 steps, 0 failures.

### Implementation for User Story 2

- [x] T014 [US2] Run `cargo test --workspace` in `VEN/` and confirm all tests pass (same count as baseline) (depends on T012)
- [x] T015 [US2] Push branch to remote and deploy to Pi4-Server: `git push`, then `ssh Pi4-Server "cd /srv/docker/openadr_lab && git pull"` (depends on T014)
- [x] T016 [US2] Run BDD integration tests on Pi4-Server: `docker compose -f tests/docker-compose.test.yml run --build --rm test-runner` — confirm 895 steps, 0 failures (depends on T015)

**Checkpoint**: User Stories 1 AND 2 complete — structure is correct and all tests green.

---

## Phase 5: Polish & Cross-Cutting Concerns

- [x] T017 [P] Add entry to `docs/history/project_journal.md` documenting the RF-02 move: what changed, why, and any issues encountered
- [x] T018 [P] Run `cargo clippy` in `VEN/` and fix any new lint warnings introduced by the move (depends on T012)
- [x] T019 Run quickstart.md verification checklist (5 steps) to confirm all acceptance criteria are met (depends on T016)

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Setup)**: No dependencies — start immediately
- **Phase 2 (Foundational)**: Depends on Phase 1 — blocks US1 and US2
- **Phase 3 (US1)**: Depends on Phase 2 completion
- **Phase 4 (US2)**: Depends on Phase 3 (T012 clean build)
- **Phase 5 (Polish)**: Depends on Phase 4 completion

### User Story Dependencies

- **US1 (P1)**: Can start after Phase 2 — no dependency on US2
- **US2 (P2)**: Depends on US1 T012 (clean build required before tests can pass)

### Within Phase 2 (Parallel Opportunities)

T003–T007 (five asset files) can all be created simultaneously — they are in different files with
no dependencies on each other. T002 (mod.rs) is also independent of them.

T008 (simulator/mod.rs update) depends on T002–T007 existing.
T009 (main.rs mod declaration) depends on T002.

---

## Parallel Example: Phase 2 file creation

```
# All six new asset files can be created simultaneously:
T002: Create VEN/src/assets/mod.rs
T003: Create VEN/src/assets/pv.rs
T004: Create VEN/src/assets/battery.rs
T005: Create VEN/src/assets/ev.rs
T006: Create VEN/src/assets/heater.rs
T007: Create VEN/src/assets/base_load.rs

# Then, once T002–T007 are done:
T008: Update simulator/mod.rs (re-export bridge)
T009: Add mod assets; to main.rs
```

---

## Implementation Strategy

### MVP (US1 only — proves the move is safe)

1. Phase 1: Verify baseline
2. Phase 2: Create `assets/` module (T002–T009)
3. Phase 3: Build, fix, delete old path (T010–T013)
4. **STOP and VALIDATE**: `cargo build` clean, old path gone — US1 delivered

### Full delivery

5. Phase 4: Tests (T014–T016) — confirm zero regressions
6. Phase 5: Polish (T017–T019)

---

## Notes

- [P] tasks touch different files — safe to run in parallel within the same phase
- T003–T007 are verbatim copies — no content edits expected; the compiler confirms correctness
- T010 is the only task likely to require iteration; let the compiler guide each fix
- Never delete `simulator/assets/` (T011) before the build is clean (T010)
- Always pass `--build` to `docker compose run` (T016) — test image bakes source at build time
