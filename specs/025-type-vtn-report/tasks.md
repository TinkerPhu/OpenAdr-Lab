# Tasks: Type the VTN Report Interface

**Input**: Design documents from `specs/025-type-vtn-report/`  
**Prerequisites**: plan.md ✓, spec.md ✓, research.md ✓, data-model.md ✓, contracts/ ✓, quickstart.md ✓

**Tests**: Included — explicitly required by FR-011 (contract tests), FR-012 (reporter unit tests), and Constitution II (BDD scenario for POST /reports echo-back behavior change).

**Organization**: Tasks grouped by user story. Compiler-driven approach — add structs, update trait, let `rustc` guide remaining fixes.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: User story this task belongs to (US1, US2, US3)

---

## Phase 1: Setup

**Purpose**: Confirm the baseline compiles cleanly on the feature branch before any changes.

- [x] T001 Verify `cargo check -p ven` passes clean on branch `025-type-vtn-report` before any changes

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Define the four typed structs and update the `VtnPort` trait signature. Everything else depends on this. Add contract tests while structs are fresh.

**⚠️ CRITICAL**: No user story work can begin until T002 and T003 are complete.

- [x] T002 Add `OadrReportBody`, `OadrReportResource`, `OadrReportInterval`, `OadrReportPayload` structs (with serde derives and `skip_serializing_if` on optional fields) in `VEN/src/controller/vtn_port.rs` — see `data-model.md` for exact field definitions
- [x] T003 Update `VtnPort::upsert_report` trait signature to `async fn upsert_report(&self, body: OadrReportBody) -> Result<()>` in `VEN/src/controller/vtn_port.rs` — crate will not compile until all impls are fixed
- [x] T004 [P] Add `OadrReportBody` serde contract tests (round-trip structural equality + absent-`eventID` serialization) to the `tests` module in `VEN/src/controller/vtn_port.rs` — FR-011

**Checkpoint**: Structs exist, trait is updated, contract tests are written. Compiler errors now identify every site to fix.

---

## Phase 3: User Story 1 — Compiler-Driven Type Safety (Priority: P1) 🎯 MVP

**Goal**: Every public report-building function returns a typed `OadrReportBody`; every call site passes a typed value to `VtnPort::upsert_report`. The codebase compiles and all existing tests pass.

**Independent Test**: `cargo check -p ven` produces zero errors; `cargo test -p ven` is green; `grep "serde_json::Value" VEN/src/controller/reporter.rs` shows zero matches on `pub fn` lines.

### Implementation for User Story 1

- [x] T005 [US1] Update `VtnClient::upsert_report` inherent method in `VEN/src/vtn.rs`: accept `OadrReportBody`, serialize to `Value` with `serde_json::to_value(&body)?` at entry, replace `.get("reportName")` with `&body.reportName` in the 409 path, discard the `update_report` return, return `Ok(())`
- [x] T006 [US1] Remove the unused `VtnClient::submit_report` method from `VEN/src/vtn.rs` (confirmed zero call sites — see spec Assumptions: "submit_report…becomes unreachable after the refactor")
- [x] T007 [US1] Update `VtnPort for VtnClient::upsert_report` impl in `VEN/src/vtn.rs` to match new trait signature (delegates to inherent method — depends on T005 only; T006 is independent)
- [x] T008 [US1] Convert `build_measurement_report` in `VEN/src/controller/reporter.rs` to return `Option<OadrReportBody>`: replace the final `json!({...})` macro with direct `OadrReportBody { ... }` struct construction; replace `json!({...})` payload literals with `OadrReportPayload { ... }` struct construction
- [x] T009 [US1] Convert `build_measurement_reports_for_active_events` in `VEN/src/controller/reporter.rs` to return `Vec<OadrReportBody>` (trivial — collects the return of T008)
- [x] T010 [US1] Convert `build_measurement_report_for_obligation` in `VEN/src/controller/reporter.rs` to return `Option<OadrReportBody>`: replace `json!({...})` macro and update private helper `build_soc_intervals` to return `Vec<OadrReportInterval>`; replace all inline `json!({...})` interval literals in the function body with `OadrReportInterval { ... }` struct construction
- [x] T011 [US1] Convert `build_status_report` in `VEN/src/controller/reporter.rs` to return `Option<OadrReportBody>`: replace the final `json!({...})` macro with `OadrReportBody { eventID: None, ... }` struct construction
- [x] T012 [US1] Remove the now-unused `use serde_json::{json, Value};` import line from `VEN/src/controller/reporter.rs` (T008–T011 must be complete); confirm `cargo check -p ven` passes
- [x] T013 [P] [US1] Update `services/obligation.rs` call site: `report` variable is now `OadrReportBody` — type update only, no logic change, in `VEN/src/services/obligation.rs`
- [x] T014 [P] [US1] Update `tasks/planning.rs` call site: `report` variable is now `OadrReportBody` — type update only, no logic change, in `VEN/src/tasks/planning.rs`
- [x] T015 [P] [US1] Update `tasks/sim_tick/publish.rs` call site: `report` in the `for report in reports` loop is now `OadrReportBody` — type update only, no logic change, in `VEN/src/tasks/sim_tick/publish.rs`
- [x] T016 [US1] Update existing reporter.rs unit tests to use struct field access (`report.programID`, `report.resources[0].resourceName`, etc.) and add at least one struct-field assertion test per public function — FR-012 — in `VEN/src/controller/reporter.rs`

**Checkpoint**: `cargo test -p ven` is green. `grep "serde_json::Value" VEN/src/controller/reporter.rs` zero on `pub fn` lines. User Story 1 is complete.

---

## Phase 4: User Story 2 — Typed Test Assertions in MockVtn (Priority: P2)

**Goal**: `MockVtn::submitted()` returns `Vec<OadrReportBody>` so test assertions use typed field access instead of JSON indexing.

**Independent Test**: `mock.submitted()[0].reportName` compiles and the `test_mock_vtn_records_submitted_report` test passes using struct field access.

### Implementation for User Story 2

- [x] T017 [US2] Update `MockVtn` in `VEN/src/services/test_support/mock_vtn.rs`: change `submitted_reports` field to `Arc<Mutex<Vec<OadrReportBody>>>`, update `submitted()` return type to `Vec<OadrReportBody>`, update `upsert_report` trait impl to accept `OadrReportBody` and return `Result<()>`
- [x] T018 [US2] Update the two internal `MockVtn` tests in `VEN/src/services/test_support/mock_vtn.rs` from JSON indexing (`mock.submitted()[0]["reportName"]`) to struct field access (`mock.submitted()[0].reportName`)

**Checkpoint**: `cargo test -p ven` still green. `MockVtn::submitted()` returns typed values.

---

## Phase 5: User Story 3 — Typed HTTP Ingestion and Echo-Back (Priority: P3)

**Goal**: `POST /reports` deserializes the request body into `OadrReportBody` (422 on missing required fields), forwards to VTN, returns `201 Created` with the submitted body echoed back.

**Independent Test**: POST a valid report body → `201` with body echoed. POST with missing `programID` → `422`. (BDD scenario T019 must fail before T020 is implemented — Constitution II.)

### Tests for User Story 3

> **⚠️ CONSTITUTION II GATE — Write this scenario FIRST; confirm it FAILS before implementing T020.**

- [x] T019 [US3] Write BDD scenario in `tests/features/ven_reports.feature` covering: (a) valid POST to `/reports` returns `201` with submitted body echoed; (b) POST with missing `programID` returns `422` — this scenario MUST fail before T020 is started

### Implementation for User Story 3

- [x] T020 [US3] Update `post_reports` handler in `VEN/src/routes/reports.rs`: change `Json(body): Json<serde_json::Value>` to `Json(body): Json<OadrReportBody>`, clone body before `upsert_report`, return `(StatusCode::CREATED, Json(body))` on `Ok(())` — see `contracts/post-reports-body.md`

**Checkpoint**: All three user stories are independently functional. BDD scenario from T019 now passes.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Verify success criteria, confirm no regressions, update project journal.

- [x] T021 [P] Run success-criteria grep checks per `quickstart.md`: `grep "serde_json::Value" VEN/src/vtn.rs` (internal only), `grep "serde_json::Value" VEN/src/controller/vtn_port.rs` (only `OadrPayload::values` and `OadrReportPayload::values`), `grep "serde_json::Value" VEN/src/controller/reporter.rs` (zero on `pub fn` lines) — SC-001
- [x] T022 Run `cargo test -p ven` and confirm all tests pass with no failures — SC-002, SC-003, SC-004
- [x] T023 Record implementation notes (approach, issues, decisions) in `docs/history/project_journal.md` — Constitution Development Workflow §1

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Phase 1 — **BLOCKS all user stories**
- **US1 (Phase 3)**: Depends on Phase 2 (T002 + T003 complete)
- **US2 (Phase 4)**: Depends on Phase 2 (T003 complete — trait changed); can overlap with late US1 tasks since it touches different files
- **US3 (Phase 5)**: Depends on Phase 2 (T002 + T003); BDD test (T019) can be written before Phase 3 completes
- **Polish (Phase 6)**: Depends on all story phases complete

### User Story Dependencies

- **US1 (P1)**: After Foundational — no dependency on US2 or US3
- **US2 (P2)**: After Foundational — no dependency on US1 or US3 (touches only `mock_vtn.rs`)
- **US3 (P3)**: After Foundational — T019 (BDD) can start immediately; T020 depends on T003 only

### Within US1 (reporter.rs tasks are sequential — same file)

```
T002 (structs) → T003 (trait change) →
  ├─ T005 → T007   (vtn.rs: VtnClient inherent → VtnPort impl)
  │   T006        (vtn.rs: remove submit_report — independent of T007)
  ├─ T008 → T009 → T010 → T011 → T012   (reporter.rs: serial, same file)
  └─ T013 [P], T014 [P], T015 [P]   (three call-site files: parallel)
T016 (reporter tests, same file as T008–T012 — after T012)
```

### Parallel Opportunities

- T004 [P]: contract tests — can be written any time after T002
- T013 [P], T014 [P], T015 [P]: three call-site files — fully parallel with each other after T012
- T021 [P]: grep checks — parallel with T022 and T023

---

## Parallel Example: User Story 1 (call sites)

```bash
# After T012 (reporter.rs complete), these three can be tackled simultaneously:
Task T013: Update VEN/src/services/obligation.rs
Task T014: Update VEN/src/tasks/planning.rs
Task T015: Update VEN/src/tasks/sim_tick/publish.rs
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup (T001)
2. Complete Phase 2: Foundational (T002–T004) — **CRITICAL, blocks everything**
3. Complete Phase 3: User Story 1 (T005–T016)
4. **STOP and VALIDATE**: `cargo test -p ven` green + SC-001 greps pass
5. US1 delivers the complete compile-time safety guarantee independently

### Incremental Delivery

1. Phase 1 + 2 → typed structs defined, baseline broken intentionally
2. Phase 3 → crate compiles again, typed everywhere, all tests green ← **MVP**
3. Phase 4 → typed test ergonomics (MockVtn)
4. Phase 5 → typed HTTP ingestion + BDD coverage
5. Phase 6 → polish, journal, SC verification

---

## Notes

- [P] tasks = different files, safe to parallelize
- [Story] label maps task to specific user story for traceability
- T019 (BDD) MUST fail before T020 (handler impl) — Constitution II gate
- reporter.rs (959 lines) already exceeds Constitution VI 500-line limit; this is a pre-existing violation not introduced by this feature — tracked in plan.md Complexity Tracking, not fixed here
- `tasks/sim_tick/publish.rs` calls `VtnClient` directly (bypasses `VtnPort` trait) — pre-existing issue, not changed here
- Commit after each checkpoint phase to preserve compiler-error-free states
