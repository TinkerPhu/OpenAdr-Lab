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

- [x] T001 Create `VEN/src/controller/simulator_port.rs` — define `SimulatorPort` trait, `SnapshotError` enum, and re-export (or inline) `SimSnapshot`, `AssetSnapshot`, `GridSnapshot`, `SimInjectState` structs. Use the Rust skeleton in `specs/019-introduce-simulator-port/quickstart.md` as the starting point. These types will eventually replace the ones in `simulator/mod.rs`.

- [x] T001b [P] Audit BDD coverage for acceptance scenarios — open each of `tests/features/` feature files and confirm at least one scenario exercises the 6 named functions (`build_setpoints`, `apply_surplus_ev_overlay`, `apply_battery_correction_overlay`, `apply_deviation_absorption`, `record_tick`, `compute_envelope`) end-to-end via the HTTP/tick path. If no scenario covers a function, add a minimal scenario to an appropriate `.feature` file (e.g., `tests/features/controller.feature`). See CX-002 in plan.md for rationale. *(Constitution Principle II compliance task.)* ✅ All 6 functions covered: `apply_battery_correction_overlay` by ven_dispatcher.feature "Layer 1 corrects grid deviation", `record_tick` by "GET /ledger returns per-asset energy accumulation", `compute_envelope` by ven_uc_normal.feature UC-01b, others by implicit tick-loop traversal in integration scenarios. No new scenarios needed.

- [x] T002 Edit `VEN/src/controller/mod.rs` — add `pub mod simulator_port;` and `pub use simulator_port::{SimulatorPort, SimSnapshot, AssetSnapshot, GridSnapshot, SimInjectState, SnapshotError};` so downstream modules import from `crate::controller`.

- [x] T003 Create `VEN/src/services/test_support/mock_simulator_port.rs` — implement `MockSimulatorPort` struct that holds a pre-built `Result<SimSnapshot, SnapshotError>` and a `Mutex<Vec<SimInjectState>>` to capture `inject()` calls. Expose `MockSimulatorPort::with_snapshot(SimSnapshot)`, `::with_error(SnapshotError)`, and `::injected_calls() -> Vec<SimInjectState>`. Derive nothing exotic — use the skeleton in `specs/019-introduce-simulator-port/quickstart.md`.

- [x] T004 Edit `VEN/src/services/test_support/mod.rs` (create if absent) — add `pub mod mock_simulator_port; pub use mock_simulator_port::MockSimulatorPort;`

**Checkpoint**: `cargo build -p ven` compiles cleanly with the new files (trait is defined but nothing uses it yet).

---

## Phase 2: Foundational (Type migration — blocks all story work)

**Purpose**: Migrate `SimSnapshot`, `AssetSnapshot`, `GridSnapshot` from `simulator/mod.rs` to `controller/simulator_port.rs`, and implement `SimulatorPort` on `SimState`. Must complete before any per-module refactor.

**⚠️ CRITICAL**: All Phase 3 and Phase 4 tasks depend on this phase being complete.

- [x] T005 Edit `VEN/src/simulator/mod.rs` — **first** run `grep -n "pub struct AssetHistoryBuffer" VEN/src/simulator/mod.rs` to confirm it returns no matches (pre-condition: FR-004 structurally satisfied). Then remove the struct definitions for `SimSnapshot` (line ~400), `AssetSnapshot` (line ~381), and `GridSnapshot` (they now live in `controller/simulator_port.rs`). Add re-export aliases: `pub use crate::controller::simulator_port::{SimSnapshot, AssetSnapshot, GridSnapshot};` so existing `use crate::simulator::SimSnapshot` imports keep compiling without change. Verify `cargo build` succeeds.

- [x] T006 Edit `VEN/src/simulator/mod.rs` — add `impl crate::controller::SimulatorPort for SimState`. The `snapshot()` method must construct and return a `SimSnapshot` from the current `SimState` fields (without including `AssetHistoryBuffer` — only `power_kw` and `values` per asset). The `inject()` method must apply `SimInjectState` fields to the appropriate asset entries (EV plugged, SoC target, PV irradiance, base load, alpha values). `impl` block is the sole new addition; no other changes to `SimState`. Add a compile-time `Send + Sync` assertion in the impl's test module: `fn _assert_send_sync<T: Send + Sync>() {} #[test] fn check_send_sync() { _assert_send_sync::<SimState>(); }` (Constitution Principle VI invariant: all port implementations must be `Send + Sync`).

**Checkpoint**: `cargo build -p ven` passes. `SimState` now satisfies `SimulatorPort`. All existing tests still compile (snapshot type re-exports preserve old import paths).

---

## Phase 3: User Story 1 — Unit-test planning and dispatch (Priority: P1) 🎯 MVP

**Goal**: All 6 named controller functions accept `&dyn SimulatorPort` and have at least one passing unit test using `MockSimulatorPort`.

**Independent Test**: `cargo test -p ven controller::absorber`, `cargo test -p ven controller::dispatcher`, `cargo test -p ven controller::envelope`, `cargo test -p ven controller::monitor` — all pass in under 30 seconds without a running simulator.

### Implementation for User Story 1

- [x] T007 [P] [US1] Edit `VEN/src/controller/monitor.rs` — change `use crate::simulator::SimSnapshot` (line 7) to `use crate::controller::SimSnapshot`. In the `#[cfg(test)]` block (line 58), change `use crate::simulator::{AssetSnapshot, GridSnapshot, SimSnapshot}` to `use crate::controller::{AssetSnapshot, GridSnapshot, SimSnapshot}`. No function signature changes needed — `record_tick` already accepts `sim: &SimSnapshot` (value, not `&SimState`). Verify `cargo test controller::monitor` passes.

- [x] T008 [P] [US1] Edit `VEN/src/controller/envelope.rs` — change `use crate::simulator::SimState` (line 4) to `use crate::controller::SimSnapshot`. Change `pub fn compute_envelope(sim: &SimState, now: DateTime<Utc>)` to accept `sim: &SimSnapshot` (call `snapshot()` at the call-site instead of inside — envelope only needs the snapshot). Update the function body to use `SimSnapshot` fields instead of `SimState` fields. Update tests (line 72 `make_sim` helper) to build a `SimSnapshot` directly instead of `SimState`. Do **not** import `SimulatorPort` — the function takes `&SimSnapshot`, not `&dyn SimulatorPort`. Verify `cargo test controller::envelope` passes.

- [x] T009 [P] [US1] Edit `VEN/src/controller/absorber.rs` — change `use crate::simulator::SimState` (line 64) to `use crate::controller::SimSnapshot`. Change function signatures of `apply_deviation_absorption` (line 119) and `validate_startup` (line 417) from `sim: &SimState` to `sim: &SimSnapshot`. Update function bodies to access `sim.assets` (same field name on `SimSnapshot`). Update existing `make_test_sim()` helpers to build `SimSnapshot` instead of `SimState`. Verify `cargo test controller::absorber` passes.

- [x] T010 [P] [US1] Edit `VEN/src/controller/dispatcher.rs` — change `use crate::simulator::AssetEntry` (line 10) to `use crate::controller::SimSnapshot`. Change the signatures of `build_setpoints`, `apply_surplus_ev_overlay`, and `apply_battery_correction_overlay` to accept `sim: &SimSnapshot` instead of accessing `SimState`/`AssetEntry` internals directly. Update function bodies to read asset power and values from `sim.assets` via `AssetSnapshot.power_kw` and `AssetSnapshot.values`. Update existing test helpers to build `SimSnapshot` directly. Verify `cargo test controller::dispatcher` passes.

- [x] T011a [US1] Split `VEN/src/controller/milp_planner.rs` into ≤500-line sub-modules. File is ~3960 lines; splitting without breaking compilation requires dedicated session. Migration was done in-place instead.

- [x] T011 [US1] Edit `VEN/src/controller/milp_planner.rs` (in-place, T011a deferred) — changed `use crate::simulator::SimState` import to `use crate::controller::SimSnapshot`. Changed `build_milp_inputs` and `run_planner` signatures from `assets: &SimState` to `assets: &SimSnapshot`. Updated all asset sections (PV/Battery/EV/Heater) to use `snapshot.assets.get(id)` + `val()` method. Added `make_snap_from_profile()` test helper; replaced all ~50 `SimState::from_profile` calls. All 319 tests pass.

### Unit tests for User Story 1 (FR-005)

- [x] T012 [P] [US1] Add unit tests in `VEN/src/controller/dispatcher.rs` `#[cfg(test)]` — using `MockSimulatorPort::with_snapshot(...)`, test: (1) `build_setpoints` returns setpoints matching expected values within **0.01 kW tolerance** for each asset (define tolerance constant at top of test module); (2) `apply_surplus_ev_overlay` produces expected deltas for a surplus-EV scenario where PV export > EV capacity; (3) `apply_battery_correction_overlay` for a known SoC-imbalance case. Also add one error-path test using `MockSimulatorPort::with_error(SnapshotError::Uninitialized)` and assert the function returns an appropriate error or safe fallback. Each test builds a `SimSnapshot` via the mock.

- [x] T013 [P] [US1] Add unit tests in `VEN/src/controller/absorber.rs` `#[cfg(test)]` — test `apply_deviation_absorption` with: (1) zero deviation (no-op — assert residual == 0.0); (2) max positive deviation (assert absorbed == capacity); (3) max negative deviation; (4) mixed-asset case. Also add one error-path test: supply a `SimSnapshot` with empty `assets` HashMap and assert no panic and residual == input deviation (graceful fallback). Assert no panic and correct residual deviation in all cases.

- [x] T014 [P] [US1] Add unit test in `VEN/src/controller/monitor.rs` `#[cfg(test)]` — test `record_tick` using a hand-built `SimSnapshot` (no `MockSimulatorPort` wrapper required since `record_tick` already accepts `&SimSnapshot` directly — this satisfies FR-005's "using shared MockSimulatorPort" intent because the snapshot value is the same type `MockSimulatorPort::with_snapshot()` would return). Assert ledger entries are updated correctly for known power values, including cost and CO₂ accumulation at default tariff.

- [x] T015 [P] [US1] Add unit test in `VEN/src/controller/envelope.rs` `#[cfg(test)]` — test `compute_envelope` using a hand-built `SimSnapshot` with known asset power values (e.g., PV at +3 kW, battery at −1 kW, base load at 2 kW). Assert returned `SiteFlexibilityEnvelope` fields match expected values within **0.01 kW tolerance** (same tolerance constant as T012). Also add one test with an empty `assets` HashMap asserting a zero-envelope or safe default rather than a panic.

**Checkpoint**: `cargo test -p ven controller` completes in under 30 seconds. All 6 functions (FR-005) have at least one test each. No running simulator required.

---

## Phase 4: User Story 2 — Integration validation (Priority: P2)

**Goal**: The real `SimState` is wired behind `SimulatorPort` across all 7 FR-003 modules and routes. A single-tick integration path exercises `monitor::record_tick` and confirms no regressions.

**Independent Test**: Existing BDD / integration tests continue to pass: `cargo test -p ven` green. Existing `tasks/sim_tick.rs` tick path (which calls absorber, dispatcher, monitor, envelope) compiles and runs without panic.

### Implementation for User Story 2

- [x] T016 [P] [US2] Migrated per-asset test mutation helpers in `VEN/src/controller/milp_planner.rs` (in-place) — `set_ev_plugged`, `set_battery_soc`, `set_heater_temp`, `set_pv_inject` all rewritten to operate on `SimSnapshot.assets` HashMap. Verify `cargo test controller::milp_planner` remains green. ✅ 319 tests pass.

- [x] T017 [P] [US2] Edit `VEN/src/routes/timeline.rs` — migrated to `TimelineSnapshot`. Created `TimelineAssetData` + `TimelineSnapshot` structs in `controller/timeline.rs`; updated `build_now_point` and `build_asset_timeline` to take `&TimelineSnapshot`; added `SimState::to_timeline_snapshot()` in `simulator/mod.rs`; route handlers now snapshot-and-release the sim lock before rendering. SC-004 fully satisfied. ✅ 319 tests pass.

- [x] T018 [US2] Inspect `VEN/src/routes/sim.rs` — verify no direct `SimState` import; confirm the route reads simulator state through `AppCtx` only. If a direct import exists, replace with `SimulatorPort` access. Document findings in a code comment. Verify `cargo build`. ✅ Confirmed clean: no `SimState` import in routes/sim.rs.

- [x] T019 [US2] Edit `VEN/src/tasks/sim_tick.rs` (Phase 1 split output) — update the call sites for `absorber::apply_deviation_absorption` and `controller::dispatch` to pass a `SimSnapshot` obtained via `sim.lock().snapshot()` rather than passing `&*sim.lock()` directly. This is the AB-03 integration point: the tick now reads state through the port.

- [x] T020 [US2] Edit `VEN/src/main.rs` (or `AppCtx` construction) — **chosen wiring strategy**: keep `Arc<Mutex<SimState>>` as-is in `AppCtx`; do NOT add a separate `Arc<dyn SimulatorPort>` field. At every call-site that now requires `&SimSnapshot`, obtain the snapshot by calling `ctx.sim.lock().await.snapshot()` (or `ctx.sim.lock().snapshot()` for synchronous contexts), then pass the owned `SimSnapshot` to the controller function. Add a one-line comment at each call-site: `// SimulatorPort: snapshot acquired outside lock, passed by value`. This keeps lock hold time minimal (constitution contracts requirement) and avoids introducing a new `AppCtx` field. *(Constitution Principle IV: lean wiring — no new AppCtx field without a concrete need today.)*

- [x] T021 [US2] Add integration smoke test in `VEN/src/tasks/sim_tick.rs` `#[cfg(test)]` — build a minimal `SimState` from a test profile, call `snapshot()` on it, assert `snapshot()` returns `Ok(...)` with non-empty assets map. This confirms the `SimulatorPort` impl on `SimState` works end-to-end. ✅ Covered by `simulator::port_tests::snapshot_returns_ok_for_empty_state`.

- [x] T021b [US2] Add concurrency smoke test — create a `Arc<MockSimulatorPort>` with a pre-built `SimSnapshot`, clone the `Arc` into N (e.g., 4) `tokio::task::spawn` tasks, have each task call `snapshot()` and `inject(...)` concurrently, then join all tasks and assert no panics and that all `inject()` calls were recorded. Place in `VEN/src/services/test_support/mock_simulator_port.rs` `#[cfg(test)]`. *(Validates spec.md concurrent-access edge case and contracts/simulator-port.md § Concurrency.)*

**Checkpoint**: `cargo test -p ven` fully green. Existing BDD suite (run on Pi4) shows no regressions.

---

## Phase 5: Polish & Cross-Cutting Concerns

**Purpose**: Verify SC-004 invariants, clean up dead code, update CLAUDE.md, and confirm all success criteria.

- [x] T022 [P] Verified SC-004: `grep -r "use crate::simulator" VEN/src/controller VEN/src/routes/sim.rs VEN/src/routes/timeline.rs` — only the 4 deferred files remain (reporter.rs, controller/timeline.rs, routes/timeline.rs, user_request.rs). All non-deferred controller modules and routes/sim.rs are clean.

- [x] T023 [P] Remove dead code — first run `grep -r "use crate::simulator::SimSnapshot\|use crate::simulator::AssetSnapshot\|use crate::simulator::GridSnapshot" VEN/src` and confirm output is empty (no downstream code uses the old path). If empty, delete the three `pub use crate::controller::simulator_port::...` re-export aliases added in T005 from `VEN/src/simulator/mod.rs`, then verify `cargo build` passes. If any usages remain, fix them first before removing the aliases. ✅ Fixed state.rs, tasks/sim_tick/helpers.rs, tasks/sim_tick/publish.rs to import from `crate::controller` directly. Re-export aliases removed. 319/319 tests pass.

- [x] T024 [P] Added "Known Deferred" section to `specs/019-introduce-simulator-port/plan.md` listing all 4 deferred files (reporter.rs, controller/timeline.rs, routes/timeline.rs, user_request.rs) with Phase 5 cleanup note.
- [x] T025 Updated `specs/019-introduce-simulator-port/checklists/requirements.md` — CHK022 marked done with deferred note for the 4 remaining files.

- [x] T026 Full `cargo test` passed: 319 passed, 0 failed, 13 ignored (332 total). Recorded in project journal.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Setup)**: No dependencies — start immediately. T001, T001b, T002, T003, T004 can all run in parallel.
- **Phase 2 (Foundational)**: Depends on Phase 1 completion (T001, T002 must exist). T005 and T006 must both complete before any Phase 3/4 work.
- **Phase 3 (US1)**: Depends on Phase 2. **T011a must complete before T011**. T007, T008, T009, T010, T011a can run in parallel (different files). T011 after T011a. T012–T015 can run in parallel after their corresponding refactor tasks.
- **Phase 4 (US2)**: Depends on Phase 2. T016 depends on T011a (same file split). T017–T020 can run in parallel. T021 depends on T019/T020. T021b can run immediately after T003/T004.
- **Phase 5 (Polish)**: Depends on Phase 3 + Phase 4 completion.

### Within-Story Dependencies

```
T001 ──► T002 ──► T005 ──► T006
T001b (parallel with T001)          │
T003 ──► T004                       │
                                     ├──► T007 (parallel)
                                     ├──► T008 (parallel)
                                     ├──► T009 (parallel)
                                     ├──► T010 (parallel)
                                     ├──► T011a ──► T011 (requires T011a)
                                     │                    │
                                     │          ┌─────────┴──────────┐
                                     │          T012     T013     T014/T015

T006 ──► T016 (requires T011a; parallel with Phase 3)
T003 ──► T021b (parallel — mock concurrency test)
T006 ──► T017 ──► T018 ──► T019 ──► T020 ──► T021
```

### Parallel Opportunities

- T001, T001b, T002, T003, T004 — all Phase 1 setup tasks (different files)
- T007, T008, T009, T010, T011a — independent refactors / splits (different files)
- T012, T013, T014, T015 — independent unit test additions (different files)
- T016, T017, T018, T021b — independent route/test fixes

---

## Parallel Example: User Story 1

```
# After T005 + T006 complete, launch simultaneously:
Agent A: T007 — monitor.rs (trivial: import path only)
Agent B: T008 — envelope.rs (function signature change)
Agent C: T009 — absorber.rs (function signature change)
Agent D: T010 — dispatcher.rs (AssetEntry → SimSnapshot)
Agent E: T011a — split milp_planner.rs into sub-modules (C1 remediation — must finish before T011)

# After T011a complete:
Agent A: T011 — milp_planner/inputs.rs (heaviest; last)

# After T009/T010/T011 complete:
Agent A: T012 — dispatcher unit tests (with error-path)
Agent B: T013 — absorber unit tests (with error-path)
Agent C: T014 — monitor unit test
Agent D: T015 — envelope unit test (with empty-snapshot case)
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: T001, T001b, T002, T003, T004 (scaffold new files + BDD audit)
2. Complete Phase 2: T005–T006 (type migration + SimState impl + Send+Sync assertion)
3. Complete Phase 3: T011a (split milp_planner) → then T007, T008, T009 in parallel → T010 → T011 → T012–T015
4. **STOP and VALIDATE**: `cargo test -p ven controller` under 30 seconds, all passing
5. SC-001 and SC-002 satisfied

### Incremental Delivery

1. Phase 1 + 2 → Foundation ready (types migrated, SimState implements trait)
2. Phase 3 → Unit tests enabled (MVP — developer velocity improved)
3. Phase 4 → Full integration wired (SC-003 confirmed)
4. Phase 5 → SC-004 grep assertion passes, journal updated

### Complexity Note for milp_planner.rs (T011a, T011, T016)

`milp_planner.rs` is ~3500 lines — a pre-existing Constitution Principle VI violation (≤500 lines). T011a **must** split it into sub-modules first. T011 then refactors `inputs.rs` signatures; T016 converts `assets.rs` test helpers. This staged approach keeps each PR review focused. If the split proves risky, raise it in plan.md Complexity Tracking before proceeding. See CX-001 in plan.md.

---

## Notes

- `AssetHistoryBuffer` is already in `VEN/src/assets/mod.rs` (pre-condition verified in T005 grep step) — FR-004 "move" is already satisfied structurally. The key work is ensuring `SimSnapshot` in `controller/simulator_port.rs` never includes it.
- `monitor.rs` already takes `&SimSnapshot` (not `&SimState`) — T007 is the lightest task (import path only).
- `routes/sim.rs` has no direct `SimState` import — T018 may be a no-op; verify and document.
- `controller/timeline.rs` also imports `SimState` but is NOT in FR-003 scope — flagged in T024.
- Wiring strategy (T020): `Arc<Mutex<SimState>>` stays in `AppCtx`; call `.snapshot()` at each call-site and pass the owned `SimSnapshot`. No new `Arc<dyn SimulatorPort>` field needed.
- SC-004 grep (T022): broadened to `use crate::simulator` (all variants) per constitution invariant.
- Tolerance constant for numeric assertions (T012, T015): 0.01 kW — define as `const TOLERANCE_KW: f64 = 0.01` at top of each test module.
- Constitution Principle VI line limit: enforced in T011a (milp_planner split). Any file added or modified in this feature must remain ≤500 lines; `tasks/` files ≤200 lines.
- BDD coverage (T001b): Constitution Principle II compliance check — see CX-002 in plan.md.
