# Tasks: Introduce SimulatorPort trait (Phase 2 — AB-03)

**Input**: Design documents from `specs/019-introduce-simulator-port/`  
**Branch**: `019-introduce-simulator-port`  
**Spec**: [spec.md](spec.md) | **Plan**: [plan.md](plan.md)

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1 = P1 unit tests, US2 = P2 integration)
- Exact file paths are given; all paths relative to `VEN/src/`

---

## Phase 1: Setup (New module scaffolding)

**Purpose**: Create the new files and module wiring needed before any refactoring can begin.

- [ ] T001 Create `VEN/src/controller/simulator_port.rs` — define `SimulatorPort` trait, `SnapshotError` enum, and re-export (or inline) `SimSnapshot`, `AssetSnapshot`, `GridSnapshot`, `SimInjectState` structs. Use the Rust skeleton in `specs/019-introduce-simulator-port/quickstart.md` as the starting point. These types will eventually replace the ones in `simulator/mod.rs`.

- [ ] T002 Edit `VEN/src/controller/mod.rs` — add `pub mod simulator_port;` and `pub use simulator_port::{SimulatorPort, SimSnapshot, AssetSnapshot, GridSnapshot, SimInjectState, SnapshotError};` so downstream modules import from `crate::controller`.

- [ ] T003 Create `VEN/src/services/test_support/mock_simulator_port.rs` — implement `MockSimulatorPort` struct that holds a pre-built `Result<SimSnapshot, SnapshotError>` and a `Mutex<Vec<SimInjectState>>` to capture `inject()` calls. Expose `MockSimulatorPort::with_snapshot(SimSnapshot)`, `::with_error(SnapshotError)`, and `::injected_calls() -> Vec<SimInjectState>`. Derive nothing exotic — use the skeleton in `specs/019-introduce-simulator-port/quickstart.md`.

- [ ] T004 Edit `VEN/src/services/test_support/mod.rs` (create if absent) — add `pub mod mock_simulator_port; pub use mock_simulator_port::MockSimulatorPort;`

**Checkpoint**: `cargo build -p ven` compiles cleanly with the new files (trait is defined but nothing uses it yet).

---

## Phase 2: Foundational (Type migration — blocks all story work)

**Purpose**: Migrate `SimSnapshot`, `AssetSnapshot`, `GridSnapshot` from `simulator/mod.rs` to `controller/simulator_port.rs`, and implement `SimulatorPort` on `SimState`. Must complete before any per-module refactor.

**⚠️ CRITICAL**: All Phase 3 and Phase 4 tasks depend on this phase being complete.

- [ ] T005 Edit `VEN/src/simulator/mod.rs` — remove the struct definitions for `SimSnapshot` (line ~400), `AssetSnapshot` (line ~381), and `GridSnapshot` (they now live in `controller/simulator_port.rs`). Add re-export aliases: `pub use crate::controller::simulator_port::{SimSnapshot, AssetSnapshot, GridSnapshot};` so existing `use crate::simulator::SimSnapshot` imports keep compiling without change. Verify `cargo build` succeeds.

- [ ] T006 Edit `VEN/src/simulator/mod.rs` — add `impl crate::controller::SimulatorPort for SimState`. The `snapshot()` method must construct and return a `SimSnapshot` from the current `SimState` fields (without including `AssetHistoryBuffer` — only `power_kw` and `values` per asset). The `inject()` method must apply `SimInjectState` fields to the appropriate asset entries (EV plugged, SoC target, PV irradiance, base load, alpha values). `impl` block is the sole new addition; no other changes to `SimState`.

**Checkpoint**: `cargo build -p ven` passes. `SimState` now satisfies `SimulatorPort`. All existing tests still compile (snapshot type re-exports preserve old import paths).

---

## Phase 3: User Story 1 — Unit-test planning and dispatch (Priority: P1) 🎯 MVP

**Goal**: All 6 named controller functions accept `&dyn SimulatorPort` and have at least one passing unit test using `MockSimulatorPort`.

**Independent Test**: `cargo test -p ven controller::absorber`, `cargo test -p ven controller::dispatcher`, `cargo test -p ven controller::envelope`, `cargo test -p ven controller::monitor` — all pass in under 30 seconds without a running simulator.

### Implementation for User Story 1

- [ ] T007 [P] [US1] Edit `VEN/src/controller/monitor.rs` — change `use crate::simulator::SimSnapshot` (line 7) to `use crate::controller::SimSnapshot`. In the `#[cfg(test)]` block (line 58), change `use crate::simulator::{AssetSnapshot, GridSnapshot, SimSnapshot}` to `use crate::controller::{AssetSnapshot, GridSnapshot, SimSnapshot}`. No function signature changes needed — `record_tick` already accepts `sim: &SimSnapshot` (value, not `&SimState`). Verify `cargo test controller::monitor` passes.

- [ ] T008 [P] [US1] Edit `VEN/src/controller/envelope.rs` — change `use crate::simulator::SimState` (line 4) to `use crate::controller::{SimulatorPort, SimSnapshot}`. Change `pub fn compute_envelope(sim: &SimState, now: DateTime<Utc>)` to accept `sim: &SimSnapshot` (call `snapshot()` at the call-site instead of inside — envelope only needs the snapshot). Update the function body to use `SimSnapshot` fields instead of `SimState` fields. Update tests (line 72 `make_sim` helper) to build a `SimSnapshot` directly instead of `SimState`. Verify `cargo test controller::envelope` passes.

- [ ] T009 [P] [US1] Edit `VEN/src/controller/absorber.rs` — change `use crate::simulator::SimState` (line 64) to `use crate::controller::SimSnapshot`. Change function signatures of `apply_deviation_absorption` (line 119) and `validate_startup` (line 417) from `sim: &SimState` to `sim: &SimSnapshot`. Update function bodies to access `sim.assets` (same field name on `SimSnapshot`). Update existing `make_test_sim()` helpers to build `SimSnapshot` instead of `SimState`. Verify `cargo test controller::absorber` passes.

- [ ] T010 [US1] Edit `VEN/src/controller/dispatcher.rs` — change `use crate::simulator::AssetEntry` (line 10) to `use crate::controller::SimSnapshot`. Change the signatures of `build_setpoints`, `apply_surplus_ev_overlay`, and `apply_battery_correction_overlay` to accept `sim: &SimSnapshot` instead of accessing `SimState`/`AssetEntry` internals directly. Update function bodies to read asset power and values from `sim.assets` via `AssetSnapshot.power_kw` and `AssetSnapshot.values`. Update existing test helpers to build `SimSnapshot` directly. Verify `cargo test controller::dispatcher` passes.

- [ ] T011 [US1] Edit `VEN/src/controller/milp_planner.rs` — change `use crate::simulator::SimState` (line 32) to `use crate::controller::SimSnapshot`. Change `build_milp_inputs` and `build_now_assets` (and any function that currently takes `assets: &SimState`, line ~306) to accept `assets: &SimSnapshot`. Update internal field accesses to use `SimSnapshot.assets` HashMap. Update existing test helpers — replace `SimState::from_profile(&profile)` with `SimSnapshot`-building helpers (or call `sim.snapshot().unwrap()` in tests that still use `SimState` via `SimulatorPort`). This is the most complex task; touch only the function signatures and their callers within this file. Verify `cargo test controller::milp_planner` passes.

### Unit tests for User Story 1 (FR-005)

- [ ] T012 [P] [US1] Add unit tests in `VEN/src/controller/dispatcher.rs` `#[cfg(test)]` — using `MockSimulatorPort::with_snapshot(...)`, test `build_setpoints` returns correct setpoints for a deterministic snapshot; test `apply_surplus_ev_overlay` produces expected deltas for a surplus-EV scenario; test `apply_battery_correction_overlay` for at least one correction case. Each test builds a `SimSnapshot` via the mock.

- [ ] T013 [P] [US1] Add unit tests in `VEN/src/controller/absorber.rs` `#[cfg(test)]` — test `apply_deviation_absorption` with edge deviation values: zero deviation (no-op), max positive deviation, max negative deviation, and a mixed-asset case. Assert no panic and correct residual deviation.

- [ ] T014 [P] [US1] Add unit test in `VEN/src/controller/monitor.rs` `#[cfg(test)]` — test `record_tick` using a hand-built `SimSnapshot` (no mock needed since `record_tick` already takes `&SimSnapshot`). Assert ledger entries are updated correctly for known power values.

- [ ] T015 [P] [US1] Add unit test in `VEN/src/controller/envelope.rs` `#[cfg(test)]` — test `compute_envelope` using a hand-built `SimSnapshot` with known asset power values. Assert returned `SiteFlexibilityEnvelope` fields match expected values.

**Checkpoint**: `cargo test -p ven controller` completes in under 30 seconds. All 6 functions (FR-005) have at least one test each. No running simulator required.

---

## Phase 4: User Story 2 — Integration validation (Priority: P2)

**Goal**: The real `SimState` is wired behind `SimulatorPort` across all 7 FR-003 modules and routes. A single-tick integration path exercises `monitor::record_tick` and confirms no regressions.

**Independent Test**: Existing BDD / integration tests continue to pass: `cargo test -p ven` green. Existing `tasks/sim_tick.rs` tick path (which calls absorber, dispatcher, monitor, envelope) compiles and runs without panic.

### Implementation for User Story 2

- [ ] T016 [P] [US2] Edit `VEN/src/controller/milp_planner.rs` (existing tests) — update the test helpers `set_ev_plugged`, `set_battery_soc`, `set_heater_temp`, `set_pv_inject` (which mutate `SimState` directly) to instead build a new `SimSnapshot` with the modified values. This unblocks the remaining milp integration tests from needing a mutable `SimState`.

- [ ] T017 [P] [US2] Edit `VEN/src/routes/timeline.rs` — change `use crate::simulator::SimState` (line 10). Inspect how `SimState` is accessed in route handlers; replace direct `sim` access with a call to `ctx.sim.lock().snapshot()` (or equivalent) to obtain a `SimSnapshot`, then pass the snapshot to `controller::timeline` functions. Verify `cargo build`.

- [ ] T018 [US2] Inspect `VEN/src/routes/sim.rs` — verify no direct `SimState` import; confirm the route reads simulator state through `AppCtx` only. If a direct import exists, replace with `SimulatorPort` access. Document findings in a code comment. Verify `cargo build`.

- [ ] T019 [US2] Edit `VEN/src/tasks/sim_tick.rs` (Phase 1 split output) — update the call sites for `absorber::apply_deviation_absorption` and `controller::dispatch` to pass a `SimSnapshot` obtained via `sim.lock().snapshot()` rather than passing `&*sim.lock()` directly. This is the AB-03 integration point: the tick now reads state through the port.

- [ ] T020 [US2] Edit `VEN/src/main.rs` (or `AppCtx` construction) — ensure `Arc<dyn SimulatorPort>` is wired from the `SimState` instance to all modules that now accept `&dyn SimulatorPort`. If `AppCtx` holds `Arc<Mutex<SimState>>`, the existing lock-then-call pattern satisfies `SimulatorPort` without an additional `Arc<dyn SimulatorPort>` wrapper. Document the chosen wiring strategy with a brief comment.

- [ ] T021 [US2] Add integration smoke test in `VEN/src/tasks/sim_tick.rs` `#[cfg(test)]` — build a minimal `SimState` from a test profile, call `snapshot()` on it, assert `snapshot()` returns `Ok(...)` with non-empty assets map. This confirms the `SimulatorPort` impl on `SimState` works end-to-end.

**Checkpoint**: `cargo test -p ven` fully green. Existing BDD suite (run on Pi4) shows no regressions.

---

## Phase 5: Polish & Cross-Cutting Concerns

**Purpose**: Verify SC-004 invariants, clean up dead code, update CLAUDE.md, and confirm all success criteria.

- [ ] T022 [P] Verify SC-004: run `grep -r "use crate::simulator::SimState" VEN/src/controller VEN/src/routes/sim.rs VEN/src/routes/timeline.rs` — assert output is empty. Fix any remaining direct imports.

- [ ] T023 [P] Remove dead code: delete the original `SimSnapshot`, `AssetSnapshot`, `GridSnapshot` struct definitions from `VEN/src/simulator/mod.rs` once the re-export aliases are confirmed working (from T005). Remove the re-export aliases too if no downstream code uses the old path.

- [ ] T024 [P] Verify `controller/timeline.rs` — it also imports `use crate::simulator::SimState` (out of FR-003 scope but a related breach). File a note in `specs/019-introduce-simulator-port/plan.md` under a "Known Deferred" section for cleanup in Phase 5 of the overall refactor, or fix here if trivial.

- [ ] T025 Update `specs/019-introduce-simulator-port/checklists/requirements.md` — mark CHK019 (`tasks.md` generated), CHK020–CHK022 (unit tests passing, integration tests green, grep assertion passing) as done.

- [ ] T026 Run full `cargo test -p ven` and confirm pass. Record result in the project journal at `docs/history/project_journal.md`.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Setup)**: No dependencies — start immediately. T001–T004 can all run in parallel.
- **Phase 2 (Foundational)**: Depends on Phase 1 completion (T001, T002 must exist). T005 and T006 must both complete before any Phase 3/4 work.
- **Phase 3 (US1)**: Depends on Phase 2. T007–T011 can run in parallel (different files). T012–T015 can run in parallel after their corresponding refactor tasks.
- **Phase 4 (US2)**: Depends on Phase 2. T016 can start in parallel with Phase 3. T017–T020 can run in parallel. T021 depends on T019/T020.
- **Phase 5 (Polish)**: Depends on Phase 3 + Phase 4 completion.

### Within-Story Dependencies

```
T001 ──► T002 ──► T005 ──► T006
T003 ──► T004               │
                             ├──► T007 (parallel)
                             ├──► T008 (parallel)
                             ├──► T009 (parallel)
                             ├──► T010
                             └──► T011 (heaviest; last)
                                      │
                             ┌────────┴──────────┐
                             T012     T013     T014/T015

T006 ──► T016 (parallel with Phase 3)
T006 ──► T017 ──► T018 ──► T019 ──► T020 ──► T021
```

### Parallel Opportunities

- T001, T002, T003, T004 — all Phase 1 setup tasks (different files)
- T007, T008, T009 — independent controller module refactors (different files)
- T012, T013, T014, T015 — independent unit test additions (different files)
- T016, T017, T018 — independent route/test fixes

---

## Parallel Example: User Story 1

```
# After T005 + T006 complete, launch simultaneously:
Agent A: T007 — monitor.rs (trivial: import path only)
Agent B: T008 — envelope.rs (function signature change)
Agent C: T009 — absorber.rs (function signature change)

# After T009/T010 complete:
Agent A: T012 — dispatcher unit tests
Agent B: T013 — absorber unit tests
Agent C: T014 — monitor unit test (already takes &SimSnapshot)
Agent D: T015 — envelope unit test
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: T001–T004 (scaffold new files)
2. Complete Phase 2: T005–T006 (type migration + SimState impl)
3. Complete Phase 3 in order: T007 → T008 → T009 → T010 → T011, then T012–T015
4. **STOP and VALIDATE**: `cargo test -p ven controller` under 30 seconds, all passing
5. SC-001 and SC-002 satisfied

### Incremental Delivery

1. Phase 1 + 2 → Foundation ready (types migrated, SimState implements trait)
2. Phase 3 → Unit tests enabled (MVP — developer velocity improved)
3. Phase 4 → Full integration wired (SC-003 confirmed)
4. Phase 5 → SC-004 grep assertion passes, journal updated

### Complexity Note for milp_planner.rs (T010, T011, T016)

`milp_planner.rs` is ~3500 lines with dozens of existing tests that build `SimState::from_profile`. These tests will need their helper functions converted from `SimState` builders to `SimSnapshot` builders. Tackle T011 last within Phase 3; do T010/T016 as separate focused PRs if the changeset grows large.

---

## Notes

- `AssetHistoryBuffer` is already in `VEN/src/assets/mod.rs` — FR-004 "move" is already satisfied structurally. The key work is ensuring `SimSnapshot` in `controller/simulator_port.rs` never includes it.
- `monitor.rs` already takes `&SimSnapshot` (not `&SimState`) — T007 is the lightest task.
- `routes/sim.rs` has no direct `SimState` import — T018 may be a no-op; verify and document.
- `controller/timeline.rs` also imports `SimState` but is NOT in FR-003 scope — flagged in T024.
- Constitution Principle VI verifiable invariant: `grep -r "use crate::simulator" VEN/src/controller VEN/src/routes/sim.rs VEN/src/routes/timeline.rs` must return empty after Phase 5.
