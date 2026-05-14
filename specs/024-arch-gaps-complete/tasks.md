# Tasks: Complete VEN Architecture Gaps

**Input**: Design documents from `/specs/024-arch-gaps-complete/`
**Branch**: `024-arch-gaps-complete`

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: User story label (US1–US6 map to spec.md user stories)
- Exact file paths in every task

---

## Phase 1: Setup

**Purpose**: Add the single new dependency and wire the new module into the module tree.

- [x] T001 Add `async_trait = "0.1"` to `[dependencies]` in `VEN/Cargo.toml`
- [x] T002 Add `pub mod vtn_port;` to `VEN/src/controller/mod.rs`
- [x] T003 Add `pub mod mock_vtn;` to `VEN/src/services/test_support/mod.rs`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Define `VtnPort` trait + all typed VTN structs + `MockVtn`. These are required by US3 (ObligationService), US5 (VTN cascade), and must exist before any service can be wired to a typed VTN interface.

**⚠️ CRITICAL**: US3 and US5 cannot begin until this phase is complete.

- [x] T004 Create `VEN/src/controller/vtn_port.rs` with `VtnPort` trait (`fetch_programs`, `fetch_events`, `fetch_reports`, `upsert_report`); annotate with `#[async_trait::async_trait]`; add `#[allow(non_snake_case)]` at file level
- [x] T005 [P] Define `OadrProgram { id, programName }` and `OadrReport { id, reportName }` in `VEN/src/controller/vtn_port.rs`
- [x] T006 [P] Define `OadrEvent`, `OadrInterval`, `OadrIntervalPeriod`, `OadrPayload`, `OadrReportDescriptor` in `VEN/src/controller/vtn_port.rs` with all fields from data-model.md; `OadrPayload::values` stays `Vec<serde_json::Value>`
- [x] T007 Add `#[cfg(test)]` contract tests in `VEN/src/controller/vtn_port.rs`: deserialize fixture JSON string into `OadrEvent` and assert `id`, `programID`, `intervals` are populated; assert unknown fields do not panic; assert absent optional field is `None`
- [x] T008 Create `VEN/src/services/test_support/mock_vtn.rs` with `MockVtn` struct implementing `VtnPort`; store `submitted_reports: Arc<Mutex<Vec<serde_json::Value>>>` for test assertions; `fetch_events` returns configurable `Vec<OadrEvent>`

**Checkpoint**: `wsl cargo check` passes; `VtnPort` trait and all typed structs compile; `MockVtn` compiles.

---

## Phase 3: US1 — Developer tests planning logic without simulator (Priority: P1) 🎯 MVP

**Goal**: Extract `evaluate_acceptance_gate` as a pure function and `PlanningService::run_cycle` into `services/planning.rs`. Slim `tasks/planning.rs` to an orchestrator shell.

**Independent Test**: `wsl cargo test services::planning` — all 5 acceptance gate unit tests pass with no external dependencies.

- [x] T009 [US1] Create `VEN/src/services/planning.rs`; define `PlanningService` (unit struct) and extract `evaluate_acceptance_gate(current: Option<&Plan>, new_plan: &Plan, trigger: &PlanTrigger, threshold_eur: f64, decay_s: f64, now: DateTime<Utc>) -> bool` as a pure function containing the threshold/decay logic from `tasks/planning.rs` lines 218–265
- [x] T010 [US1] Add `#[cfg(test)]` block in `VEN/src/services/planning.rs` with 5 unit tests: `test_gate_rejects_below_threshold_on_periodic`, `test_gate_accepts_on_deviation_trigger`, `test_gate_accepts_when_no_current_plan`, `test_gate_accepts_after_decay_window`, `test_gate_accepts_epsilon_improvement`; all tests must pass
- [x] T011 [US1] Define `PlanCycleResult { adopted: bool, plan: Plan, solver_ms: u64 }` and `PlanningService::run_cycle(...)` in `VEN/src/services/planning.rs`; extract event emission (`SolvingStarted`, `PlanReady`, `CorrectionCleared`) and state mutation (`state.set_active_plan`, `state.set_site_envelope`) from `tasks/planning.rs` lines 95–287 into this method
- [x] T012 [US1] Slim `VEN/src/tasks/planning.rs` to ≤80 lines: retain only channel setup, sim-lock + snapshot, `asset_contexts` build, `spawn_blocking` call, `PlanningService::run_cycle` call, and the wake-trigger `select!` loop; remove all inlined acceptance gate and event emission code
- [x] T013 [US1] Export `PlanningService` in `VEN/src/services/mod.rs`

**Checkpoint**: `wsl cargo test services::planning` passes (5 tests); `wsl cargo check` passes; `tasks/planning.rs` ≤80 lines.

---

## Phase 4: US2 — Developer tests user request lifecycle without HTTP (Priority: P1)

**Goal**: Extract `UserRequestService` from `routes/hems.rs` `post_requests` handler and `delete_request` handler. Route handlers become thin adapters.

**Independent Test**: `wsl cargo test services::user_request` — all 6 unit tests pass with no HTTP dependency.

- [x] T014 [P] [US2] Create `VEN/src/services/user_request.rs`; define `UserRequestService` (unit struct) with `create_ev(body, assets, asset_configs, now) -> Result<(UserRequest, EvSession), RequestError>` — extract EV-path logic from `routes/hems.rs` lines 246–270; `create_heater` and `create_shiftable` methods analogously
- [x] T015 [P] [US2] Add `UserRequestService::cancel(id: Uuid, state: &AppState) -> Result<UserRequest>` in `VEN/src/services/user_request.rs`; sets status ABANDONED, clears linked EV/heater session from state, returns error for unknown or already-terminal requests
- [x] T016 [US2] Add `#[cfg(test)]` block in `VEN/src/services/user_request.rs` with 6 unit tests: `test_create_ev_links_session`, `test_create_shiftable_builds_load`, `test_cancel_sets_abandoned_and_clears_ev_session`, `test_cancel_unknown_id_returns_err`, `test_cancel_terminal_request_returns_err`, `test_duplicate_asset_rejected`; all tests must pass
- [x] T017 [US2] Update `VEN/src/routes/hems.rs` `post_requests` handler: replace inline EV/heater/shiftable creation logic with calls to `UserRequestService::create_*`; handler body becomes parse → discriminate → call one service method → state.upsert + trigger + 201
- [x] T018 [US2] Update `VEN/src/routes/hems.rs` `delete_request` handler to call `UserRequestService::cancel`; handler body becomes parse id → call service → map Ok/Err to 200/404
- [x] T019 [US2] Export `UserRequestService` in `VEN/src/services/mod.rs`

**Checkpoint**: `wsl cargo test services::user_request` passes (6 tests); `wsl cargo check` passes; `post_requests` and `delete_request` handlers contain no inline business logic.

---

## Phase 5: US5 — VTN responses carry typed fields (Priority: P2)

**Goal**: Update `VtnClient` to implement `VtnPort`; cascade typed structs through `PollingState` and all consumers. After this phase `grep "serde_json::Value" VEN/src/vtn.rs` returns no public method signatures.

**Independent Test**: `wsl cargo test controller::vtn_port` passes; `wsl cargo check` with no `serde_json::Value` in public `VtnPort` methods.

- [x] T020 [US5] Implement `VtnPort` for `VtnClient` in `VEN/src/vtn.rs`: `fetch_programs` deserialises the raw JSON array into `Vec<OadrProgram>`; `fetch_events` into `Vec<OadrEvent>`; `fetch_reports` into `Vec<OadrReport>`; `upsert_report` delegates to existing `upsert_report` body; annotate with `#[async_trait::async_trait]`
- [x] T021 [US5] Update `VEN/src/state.rs` `PollingState`: change `programs`, `events`, `reports` fields from `Vec<serde_json::Value>` to `Vec<OadrProgram>`, `Vec<OadrEvent>`, `Vec<OadrReport>`; update all `AppState` getter/setter methods for those three fields
- [x] T022 [P] [US5] Update `VEN/src/controller/openadr_interface.rs`: change `parse_rate_snapshots(events: &[Value], ...)` and `parse_capacity_state(events: &[Value])` signatures to `&[OadrEvent]`; replace all `.get("field")` accesses with typed struct field access (`.id`, `.intervals`, `.intervalPeriod`, etc.)
- [x] T023 [P] [US5] Update `VEN/src/tasks/poll_events.rs`: change `detect_event_changes(events: &[serde_json::Value], ...)` to `&[OadrEvent]`; replace `.get("id")`, `.get("eventName")`, `.get("intervals")` etc. with typed field access; `AppState::set_events(...)` now accepts `Vec<OadrEvent>`
- [x] T024 [P] [US5] Update `VEN/src/controller/reporter.rs`: change event parameter from `&serde_json::Value` to `&OadrEvent` in `build_measurement_report_for_obligation` and any function that receives an event directly; replace `.get("field")` accesses with typed access
- [x] T025 [US5] Update `VEN/src/routes/events.rs`: `AppState::events()` now returns `Vec<OadrEvent>`; ensure the route serialises with `Json(events)` (OadrEvent implements Serialize)
- [x] T026 [US5] Run `wsl cargo check`; fix any remaining type errors in files not covered above (e.g. callers that pass `serde_json::Value` where `OadrEvent` is now expected)

**Checkpoint**: `wsl cargo check` passes with zero type errors; `grep "serde_json::Value" VEN/src/vtn.rs` returns no public method signatures; `wsl cargo test` passes.

---

## Phase 6: US3 — Developer tests VTN polling without network (Priority: P2)

**Goal**: Create `ObligationService`, wire it into `tasks/obligation.rs`, add unit tests using `MockVtn`.

**Prerequisite**: Phase 5 must be complete (`VtnClient` implements `VtnPort`).

**Independent Test**: `wsl cargo test services::obligation` — all 3 unit tests pass using `MockVtn` with no network.

- [x] T027 [US3] Create `VEN/src/services/obligation.rs`; define `ObligationService` (unit struct) and `check_and_report(state: &AppState, sim: &Arc<Mutex<SimState>>, vtn: &dyn VtnPort, ven_name: &str, now: DateTime<Utc>) -> anyhow::Result<()>`; extract inner loop body from `tasks/obligation.rs` lines 23–58 (obligation fetch, report build, `vtn.upsert_report`, `state.mark_obligation_fulfilled`); service propagates `vtn` errors to caller without retry
- [x] T028 [US3] Add `#[cfg(test)]` block in `VEN/src/services/obligation.rs` with 3 unit tests using `MockVtn`: `test_check_submits_due_obligation` (MockVtn records the upsert call), `test_check_skips_when_none_due`, `test_check_propagates_vtn_error`; all tests must pass
- [x] T029 [US3] Update `VEN/src/tasks/obligation.rs` to delegate to `ObligationService::check_and_report`; task loop becomes: tick → call service → log error on Err, continue; target ≤30 lines
- [x] T030 [US3] Export `ObligationService` in `VEN/src/services/mod.rs`

**Checkpoint**: `wsl cargo test services::obligation` passes (3 tests); `tasks/obligation.rs` ≤30 lines.

---

## Phase 7: US4 — Developer adds HEMS command without touching route logic (Priority: P2)

**Goal**: Create `HvacService` and `EvSessionService` in `services/hems.rs`; update EV unplug and heater handlers in `routes/hems.rs` to delegate.

**Independent Test**: `wsl cargo test services::hems` — all unit tests pass with no HTTP dependency.

- [x] T031 [P] [US4] Create `VEN/src/services/hems.rs`; define `EvSessionService` (unit struct) with `end(state: &AppState) -> anyhow::Result<()>` — clears active EV session from state, transitions any linked `UserRequest` in `HemsState::active_requests` to COMPLETED if it was ACTIVE; and `HvacService` with `set_heater_target(target: HeaterTarget, state: &AppState)` and `clear_heater_target(state: &AppState)`
- [x] T032 [P] [US4] Add `#[cfg(test)]` block in `VEN/src/services/hems.rs` with unit tests: `test_ev_session_end_clears_session`, `test_ev_session_end_transitions_linked_request`, `test_heater_clear_removes_target`; all tests must pass
- [x] T033 [US4] Update `VEN/src/routes/hems.rs` EV unplug handler to call `EvSessionService::end`; heater target clear handler to call `HvacService::clear_heater_target`; each handler body becomes: parse → call service → map to HTTP response
- [x] T034 [US4] Export `HvacService` and `EvSessionService` in `VEN/src/services/mod.rs`

**Checkpoint**: `wsl cargo test services::hems` passes; `wsl cargo check` passes; EV unplug and heater clear handlers contain no inline business logic.

---

## Phase 8: US6 — tick.rs stays within module size limit (Priority: P3)

**Goal**: Extract `build_absorber_params` into `helpers.rs`; bring `tick.rs` under 200 lines.

**Independent Test**: Line count < 200; `wsl cargo test` passes.

- [x] T035 [US6] Add `pub(crate) fn build_absorber_params(profile: &Profile) -> AbsorberParams` to `VEN/src/tasks/sim_tick/helpers.rs`; function body is the struct literal from `tick.rs` lines 96–111 (enabled, dead_band_kw, dead_band_clearing_ticks, assets vec mapping)
- [x] T036 [US6] Update `VEN/src/tasks/sim_tick/tick.rs` PHASE 3: replace the inline `AbsorberParams { ... }` struct literal with `let absorber_params = super::helpers::build_absorber_params(&profile);`; verify `(Get-Content VEN/src/tasks/sim_tick/tick.rs).Count` is < 200

**Checkpoint**: `(Get-Content VEN/src/tasks/sim_tick/tick.rs).Count` < 200; `wsl cargo test` passes.

---

## Phase 9: Polish & Cross-Cutting Concerns

**Purpose**: Invariant verification, BDD validation, and documentation.

- [x] T037 [P] Run all three CLAUDE.md invariant checks: `wsl bash -c 'grep -r "use crate::profile" VEN/src/entities VEN/src/controller VEN/src/routes'` → empty; `wsl bash -c 'grep -r "use crate::assets::" VEN/src/controller/milp'` → empty; `wsl bash -c 'grep "serde_json::Value" VEN/src/vtn.rs'` → empty for public signatures; fix any violations found
- [x] T038 [P] Run `wsl cargo test 2>&1` and confirm all tests pass (zero failures); review output for new warnings introduced by this feature and fix any
- [x] T039 Run BDD suite on Pi4-Server: `ssh Pi4-Server "cd /srv/docker/openadr_lab && docker compose -f tests/docker-compose.test.yml run --build --rm test-runner"` — verify all scenarios green
- [x] T040 [P] Update `docs/history/project_journal.md` with: what was done across 3 gaps, key decisions (async_trait, minimal VTN structs, no retry in ObligationService, pure evaluate_acceptance_gate), any issues encountered

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Setup)**: No dependencies — start immediately
- **Phase 2 (Foundational)**: Depends on Phase 1
- **Phase 3 (US1 — PlanningService)**: Depends on Phase 1 only — does NOT need VtnPort
- **Phase 4 (US2 — UserRequestService)**: Depends on Phase 1 only — can run in parallel with Phase 3
- **Phase 5 (US5 — VTN typing)**: Depends on Phase 2 (VtnPort trait + structs must exist)
- **Phase 6 (US3 — ObligationService)**: Depends on Phase 2 and Phase 5 (VtnClient must implement VtnPort)
- **Phase 7 (US4 — HvacService)**: Depends on Phase 1 only — can run in parallel with Phase 5/6
- **Phase 8 (US6 — tick.rs)**: No dependencies — standalone, can run after Phase 1
- **Phase 9 (Polish)**: Depends on all phases complete

### User Story Dependencies

```
Phase 1 (Setup)
    └── Phase 2 (Foundational)
            ├── Phase 5 (US5 — VTN typing)
            │       └── Phase 6 (US3 — ObligationService)
            └── (unblocks MockVtn for all service tests)
Phase 1 (Setup)
    ├── Phase 3 (US1 — PlanningService)  ← parallel with US2
    ├── Phase 4 (US2 — UserRequestService) ← parallel with US1
    ├── Phase 7 (US4 — HvacService)  ← parallel with US3/US5
    └── Phase 8 (US6 — tick.rs)  ← truly standalone
```

### Parallel Opportunities

Within Phase 2: T005 and T006 (OadrProgram/Report vs OadrEvent structs) are in the same file so sequential.

Within Phase 4: T014 and T015 (create_ev/create_heater/create_shiftable and cancel) can be written in parallel if working in separate branches of the same file — otherwise sequential.

Within Phase 5: T022 (openadr_interface.rs), T023 (poll_events.rs), and T024 (reporter.rs) are in different files and can run in parallel after T021 (PollingState types set).

Phases 3 + 4 can run in parallel (different files entirely).
Phases 5 + 7 can run in parallel (different files entirely).

---

## Parallel Example: Phases 3 + 4 (both P1, after Phase 1)

```text
Parallel track A — US1 PlanningService:
  T009 → T010 → T011 → T012 → T013

Parallel track B — US2 UserRequestService:
  T014 + T015 (parallel) → T016 → T017 → T018 → T019
```

## Parallel Example: Phase 5 (after T021)

```text
Sequential: T020 → T021
Then parallel: T022 + T023 + T024 (different files)
Then sequential: T025 → T026
```

---

## Implementation Strategy

### MVP First (US1 only — PlanningService acceptance gate)

1. Complete Phase 1 (Setup)
2. Complete Phase 3 (US1 — PlanningService)
3. **STOP and VALIDATE**: `wsl cargo test services::planning` — 5 tests green
4. The acceptance gate is now unit-testable for the first time — this alone closes the highest-value test gap

### Incremental Delivery

1. Setup → Foundation → US1 + US2 in parallel (P1 done — planning + user request services)
2. Foundation → US5 (VTN typing cascade) → US3 (ObligationService) (typed VTN + obligation test coverage)
3. US4 (HvacService) in parallel with US3
4. US6 (tick.rs) — quick standalone fix
5. Polish — BDD green, invariants clean, journal updated

---

## Notes

- [P] tasks = different files or genuinely independent sections of the same file
- US labels map directly to user stories in spec.md
- All service files use zero-field unit structs (`pub struct ObligationService;`) — no constructor needed
- `#[allow(non_snake_case)]` is required in `vtn_port.rs` for OpenADR camelCase field names (`programID`, `eventName`, etc.) per Constitution Principle I
- `OadrPayload::values` intentionally stays `Vec<serde_json::Value>` — mixed-type array; this internal use is not in a public VtnPort signature and does not violate the invariant
- Do NOT add retry logic to `ObligationService::check_and_report` — the task loop retries on the next tick (clarified in spec)
- Do NOT type the full report body in `VtnPort::upsert_report` — only `id` + `reportName` in `OadrReport` are needed (research.md Decision 3)
