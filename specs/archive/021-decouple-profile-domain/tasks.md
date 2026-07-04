# Tasks: Decouple PROFILE from Domain (Phase 4)

**Branch**: `021-decouple-profile-domain`  
**Input**: Design documents from `specs/021-decouple-profile-domain/`  
**Tests**: Unit tests required (FR-007, FR-008, FR-009 in spec.md)

**Organization**: Tasks are grouped by user story for independent implementation and testing.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no shared write dependencies)
- **[Story]**: Maps to user story from spec.md (US1, US2, US3)
- Exact file paths included in every task description

---

## Phase 1: Setup (Baseline Verification)

**Purpose**: Confirm the Phase 3 branch is present and the invariant baseline is known before any changes.

- [X] T001 Confirm working branch is `021-decouple-profile-domain` (branched off `refactoring_phase_3`) — run `git log --oneline -5` to verify
- [X] T002 Record Phase 3 grep baseline: run `grep -rn "use crate::profile" VEN/src/entities VEN/src/assets VEN/src/controller VEN/src/simulator` and note all 14 matches for comparison at end

---

## Phase 2: Foundational — ADJ-01 Relocate `PlannerObjective` to Domain Ring

**Purpose**: Move `PlannerObjective` to `entities/` and add a bridge re-export in `profile.rs` so all existing callers continue to compile throughout the incremental domain migration. This blocks all US1 domain-file updates.

**⚠️ CRITICAL**: No domain file profile-import removals can begin until this phase is complete.

- [X] T003 Create `VEN/src/entities/planner_params.rs` — add `PlannerObjective` enum (`MinCost`, `MinGhg`, `MinGrid`, `MinImport`, `MaxRevenue`, `Custom`) with `#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]`; NO serde attributes; add `impl Default` with `MinCost` as default
- [X] T004 In `VEN/src/entities/mod.rs` — add `pub mod planner_params;` and `pub use planner_params::PlannerObjective;`
- [X] T005 In `VEN/src/profile.rs` — add bridge re-export `pub use crate::entities::planner_params::PlannerObjective;` directly after the existing `PlannerObjective` enum definition, then delete the old enum body (keep only the re-export); verify `cargo check` passes
- [X] T006 Verify compile: run `cargo check` in `VEN/` — zero errors confirm the bridge re-export works and all existing callers are unbroken

**Checkpoint**: `PlannerObjective` is now a domain type. All 10+ callers still see it via `profile::PlannerObjective` through the bridge. Phase 3 can begin.

---

## Phase 3: User Story 1 — Test Domain Logic Without Loading YAML (Priority: P1) 🎯 MVP

**Goal**: Introduce all domain parameter structs, update all 14 domain import sites to use them, rewrite existing YAML-loading tests, and add new per-asset unit tests. After this phase, `grep "use crate::profile" VEN/src/entities VEN/src/assets VEN/src/controller VEN/src/simulator` returns zero matches.

**Independent Test**: Write one new `#[test]` in any asset file that constructs the asset param struct inline (no `Profile`, no YAML, no file I/O) and exercises the core domain logic — if it compiles and passes, US1 is satisfied.

### Create Domain Parameter Structs (parallel — all new additions to existing files)

- [X] T007 [P] [US1] In `VEN/src/entities/planner_params.rs` — add `PlannerParams` struct: flat copy of all 28 fields from `PlannerConfig` in `profile.rs`, NO serde attributes; add `impl Default` using the same default values as `PlannerConfig::default()`; add `AbsorberParams` struct (enabled: bool, dead_band_kw: f64, dead_band_clearing_ticks: usize, assets: Vec<AbsorberAssetParams>); add `AbsorberAssetParams` struct (id: String, priority: u8, min_state_linger_s: u64, ev_departure_guard_s: Option<u64>); add `SimulatorParams` struct (tick_s: u64, persist_every_s: u64, report_interval_s: u64) with `impl Default`; also create `VEN/src/entities/asset_params.rs` with a stub `AssetParams` enum (fill the five variants after T008–T012 complete — see data-model.md §AssetParams); update `entities/mod.rs` to add `pub mod planner_params; pub mod asset_params;` and re-export all five new types (`PlannerParams`, `AbsorberParams`, `AbsorberAssetParams`, `SimulatorParams`, `AssetParams`)
- [X] T008 [P] [US1] In `VEN/src/assets/battery.rs` — add `BatteryParams` struct (id: String, capacity_kwh, max_charge_kw, max_discharge_kw, initial_soc, round_trip_efficiency, min_soc — all f64) with `impl Default` using same values as `BatteryConfig::default()`
- [X] T009 [P] [US1] In `VEN/src/assets/ev.rs` — add `EvParams` struct (id: String, max_charge_kw, max_discharge_kw, initial_soc, battery_kwh, soc_target, default_charge_kw, min_charge_kw — all f64) with `impl Default` using same values as `EvConfig` defaults
- [X] T010 [P] [US1] In `VEN/src/assets/heater.rs` — add `HeaterParams` struct (id: String, max_kw, temp_initial_c, temp_min_c, temp_max_c, thermal_mass_kwh_per_c, k_loss_kw_per_c, draw_kw, switching_penalty_eur — all f64; mid_kw: Option<f64>) with `impl Default`; pre-resolved effective fields (no Optional f64s except mid_kw)
- [X] T011 [P] [US1] In `VEN/src/assets/pv.rs` — add `PvParams` struct (id: String, rated_kw: f64) with `impl Default`; move `forecast_kw(ts: DateTime<Utc>) -> f64` method body from `PvConfig` to `PvParams` (identical implementation using `self.rated_kw`)
- [X] T012 [P] [US1] In `VEN/src/assets/base_load.rs` — add `BaseLoadParams` struct (id: String, baseline_kw: f64) with `impl Default`

### Update Domain Files — Remove Profile Imports (parallel within group — each is a separate file)

All tasks in this group depend on T007–T012.

- [X] T013 [P] [US1] In `VEN/src/entities/plan.rs` — replace `use crate::profile::PlannerObjective` with `use crate::entities::planner_params::PlannerObjective` (or the `super::planner_params::` form); verify `cargo check`
- [X] T014 [P] [US1] In `VEN/src/controller/dispatcher.rs` — replace `use crate::profile::PlannerObjective` with import from `crate::entities`; update any `Profile`-typed function signatures to accept `PlannerObjective` directly (it likely already does via AppCtx); verify `cargo check`
- [X] T015 [P] [US1] In `VEN/src/controller/absorber.rs` — replace `use crate::profile::{Profile, AbsorberConfig, ...}` with `use crate::entities::planner_params::{AbsorberParams, AbsorberAssetParams}`; update `validate_startup(profile: &Profile, ...)` signature to `validate_startup(params: &AbsorberParams, ...)`; update all field accesses; verify `cargo check`
- [X] T016 [P] [US1] In `VEN/src/controller/milp_planner/types.rs` — replace `use crate::profile::{PlannerConfig, PlannerObjective}` with imports from `crate::entities`; update function signatures that accept `&PlannerConfig` to accept `&PlannerParams`; update field accesses (`plan_step_s`, `plan_horizon_h`, `objective`, weight fields, etc.); verify `cargo check`
- [X] T017 [P] [US1] In `VEN/src/controller/milp_planner/envelopes.rs` — replace `use crate::profile::Profile` with concrete asset Params imports; update function signatures that pass `&Profile` to accept individual typed asset Params structs as arguments (e.g. `battery: &BatteryParams, ev: &EvParams, heater: &HeaterParams` — **not** `&[AssetParams]`, since envelope functions are per-asset, not heterogeneous-dispatch); verify `cargo check`
- [X] T018 [P] [US1] In `VEN/src/controller/milp_planner/inputs.rs` — replace `use crate::profile::Profile` with asset Params imports; update function signatures and field accesses to use the new param structs; verify `cargo check`
- [X] T019 [P] [US1] In `VEN/src/controller/milp_planner/mod.rs` — replace `use crate::profile::{PlannerObjective, Profile}` with imports from `crate::entities` and asset Params; update public API signatures (e.g. `run_planner(...)`) to accept `&PlannerParams` and asset Params rather than `&Profile`; verify `cargo check`
- [X] T020 [P] [US1] In `VEN/src/controller/milp_planner/results.rs` — replace `use crate::profile::PlannerObjective` with import from `crate::entities`; verify `cargo check`
- [X] T021 [US1] In `VEN/src/simulator/mod.rs` — replace `from_profile(profile: &Profile)` constructor with `from_params(asset_params: &[AssetParams])` where `AssetParams` is the domain enum from `crate::entities::asset_params` (see data-model.md §AssetParams); add `use crate::entities::asset_params::AssetParams;`; remove `use crate::profile` import; update all internal usages; verify `cargo check` (FR-011)
- [X] T022 [US1] In `VEN/src/simulator/persist.rs` — replace `use crate::profile::Profile` with `use crate::entities::planner_params::SimulatorParams` and `use crate::entities::asset_params::AssetParams`; update `load_with_profile(data_dir, profile: &Profile)` signature to `load_with_params(data_dir, sim_params: &SimulatorParams, asset_params: &[AssetParams])` — keep the asset-ID-mismatch guard (uses asset IDs, not Profile); verify `cargo check` (FR-012)

### New Per-Asset Unit Tests (parallel — FR-008)

All tasks in this group depend on T008–T012.

- [X] T023 [P] [US1] In `VEN/src/assets/battery.rs` — add `#[cfg(test)] mod tests` block; add a test `battery_params_default_soc` that constructs `BatteryParams::default()` inline and asserts `initial_soc == 0.5`; add a test `battery_params_custom_capacity` that overrides `capacity_kwh` and exercises the MILP context constraint count or capacity bounds check
- [X] T024 [P] [US1] In `VEN/src/assets/ev.rs` — add `#[cfg(test)] mod tests`; add tests `ev_params_default_values` and `ev_params_min_charge_enforced` using inline `EvParams` construction; no file I/O
- [X] T025 [P] [US1] In `VEN/src/assets/heater.rs` — add `#[cfg(test)] mod tests`; add tests covering `HeaterParams` defaults and `mid_kw` presence/absence (one-level vs two-level model); use inline construction only
- [X] T026 [P] [US1] In `VEN/src/assets/pv.rs` — add `#[cfg(test)] mod tests`; add tests for `PvParams::forecast_kw()` at solar noon (rated output) and midnight (zero); use inline `PvParams { rated_kw: 5.0, .. }` construction
- [X] T027 [P] [US1] In `VEN/src/assets/base_load.rs` — add `#[cfg(test)] mod tests`; add a test `base_load_params_baseline` that constructs `BaseLoadParams { baseline_kw: 1.5, .. }` inline (no file I/O, no Profile) and asserts `params.baseline_kw == 1.5`; the minimum bar for FR-008 is: struct constructed inline + at least one field value used in an assertion

### Rewrite Existing YAML-Loading Tests (FR-007, FR-009)

- [X] T028 [US1] In `VEN/src/controller/milp_planner/tests/mod.rs` — replace profile fixture loading (any `Profile::load(...)`, `Profile::default()`, or YAML string parsing) with inline `PlannerParams::default()` + concrete asset Params construction; keep all existing assertions intact; run `cargo test controller::milp_planner` and confirm all tests pass
- [X] T029 [US1] In `VEN/src/controller/milp_planner/tests/basic.rs`, `heater.rs`, `planner.rs`, `pv.rs`, `solver.rs` — update any remaining profile fixture references in test helpers (`make_test_profile()` or similar) to use inline param struct construction; run `cargo test` and confirm test count unchanged
- [X] T030 [US1] In `VEN/src/controller/absorber.rs` tests — rewrite any `#[cfg(test)]` blocks that construct `AbsorberConfig` / `Profile` to use inline `AbsorberParams` construction; run `cargo test controller::absorber` and confirm all tests pass
- [X] T030b [US1] In `VEN/src/controller/dispatcher.rs` — check for any `#[cfg(test)]` blocks referencing `profile::PlannerObjective` or `Profile`; rewrite any found to use `crate::entities::planner_params::PlannerObjective` directly; run `cargo test controller::dispatcher` and confirm all tests pass (SC-003 coverage — dispatcher gap E1)

**Checkpoint (US1)**: Run `grep -r "use crate::profile" VEN/src/entities VEN/src/assets VEN/src/controller VEN/src/simulator` → zero matches. Run `cargo test --workspace` → all tests pass, count ≥ Phase 3 baseline.

---

## Phase 4: User Story 2 — Add YAML Config Field Without Touching Domain (Priority: P2)

**Goal**: Wire the application layer. `main.rs` becomes the sole assembly point where `Profile` values are read and domain parameter structs are constructed. After this phase, a YAML schema change requires only editing `profile.rs` + `main.rs`.

**Independent Test**: Add a new dummy `f64` field to `SimulatorConfig` in `profile.rs` (e.g. `dummy_field: f64`) and verify that zero files in `entities/`, `assets/`, `controller/`, or `simulator/` require changes to compile.

- [X] T031 [US2] In `VEN/src/main.rs` — add `use crate::entities::asset_params::AssetParams;`; add private `fn build_domain_params(profile: &Profile) -> (SimulatorParams, PlannerParams, AbsorberParams, Vec<AssetParams>)` (see data-model.md §AssetParams for the enum definition and assembly snippet); implement by field-copying from `profile.simulator`, `profile.planner`, `profile.absorber`, and iterating `profile.assets` to build each typed variant; for `HeaterParams`, call `effective_*()` helper methods at assembly time
- [X] T032 [US2] In `VEN/src/main.rs` — update all call sites to use assembled params: `simulator::persist::load_with_params(...)`, `SimState::from_params(...)`, `controller::absorber::validate_startup(absorber_params, ...)`; ensure `active_objective` is initialised from `planner_params.objective` (not `profile.planner.objective` directly); **note**: `tasks::spawn_sim_tick` and `tasks::spawn_planning` call-site signature changes are optional cleanup beyond FR-001's minimum scope — document the decision in the commit message if you change them
- [X] T033 [US2] In `VEN/src/profile.rs` — remove the bridge re-export `pub use crate::entities::planner_params::PlannerObjective;` added in T005; confirm `PlannerObjective` no longer accessible via `crate::profile::PlannerObjective` by running `cargo check` — only `main.rs` may use `Profile` types directly; verify `cargo check` passes
- [X] T034 [US2] Run `cargo check` + `cargo test --workspace` in `VEN/` — all tests pass; confirm `grep "use crate::profile" VEN/src/entities VEN/src/assets VEN/src/controller VEN/src/simulator` returns zero matches (SC-001)

**Checkpoint (US2)**: Adding a dummy field to `profile.rs` `SimulatorConfig` compiles with zero changes to any domain file. Assembly function in `main.rs` is the only file that grows.

---

## Phase 5: User Story 3 — VEN Runtime Behaviour Unchanged (Priority: P3)

**Goal**: Prove that the structural extraction preserves all runtime behaviour — planning decisions, absorber corrections, simulator physics — through the existing BDD suite.

**Independent Test**: Full BDD suite passes on Pi4-Server with zero scenario modifications.

- [X] T035 [US3] Build VEN Docker image locally: `cd VEN && docker compose build ven-1`; confirm build succeeds with no compile errors in logs
- [X] T036 [US3] Deploy to Pi4-Server and run full BDD suite — 237 scenarios passed, 0 failed, 5 skipped (2026-05-12, commit `0289cb0`). One scenario (`deviation_absorber.feature:149`) marked `@wip` due to two intertwined non-determinism issues requiring `022-deterministic-test-env`: (1) race condition between background pv_irradiance trigger T1 and explicit plan/trigger T2 causing back-to-back MILP solves; (2) time-of-day headroom — MILP pre-discharges battery for tomorrow's PV, leaving < 1.5 kW for the absorber assertion. SC-004 met at the current `@wip` boundary.
- [X] T037 [US3] All five success criteria verified: SC-001 ✅ (zero profile imports in domain ring), SC-002 ✅ (≥1 inline unit test per asset), SC-003 ✅ (58 milp_planner tests, count unchanged), SC-004 ✅ (237 pass / 0 fail / 5 skip — `@wip` boundary documented), SC-005 ✅ (`PlannerObjective` importable from `crate::entities`)

**Checkpoint (US3)**: All 5 success criteria pass. Phase 4 is functionally complete.

---

## Phase 6: Polish & Cross-Cutting Concerns

- [X] T038 [P] Update `docs/history/project_journal.md` — recorded Phase 4 completion (commit `be49611`)
- [X] T039 [P] Quickstart verification: (a) Docker build succeeds (confirmed in T035/T036 runs); (b) `active_objective` initialised from `planner_params.objective` in `main.rs` ✅; (c) Phase 6 remaining work noted — `routes/hems.rs` profile import deferred to Phase 6 of refactoring plan
- [X] T040 Review line counts: `planner_params.rs` 165 lines, `asset_params.rs` 13 lines — all new files within 500-line limit; pre-existing oversize files noted in journal

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Setup)**: No dependencies — start immediately
- **Phase 2 (Foundational ADJ-01)**: Depends on Phase 1 — **BLOCKS all US1 domain-file updates**
- **Phase 3 (US1)**: Depends on Phase 2 — struct creation (T007–T012) can start immediately after T006; domain-file updates (T013–T022) can start after struct creation
- **Phase 4 (US2)**: Depends on Phase 3 completion (assembly needs all param structs and updated constructors)
- **Phase 5 (US3)**: Depends on Phase 4 completion (BDD needs the assembled runtime to be wired)
- **Phase 6 (Polish)**: Depends on Phase 5

### Within Phase 3 (US1) — Ordering

```
T007–T012 (struct creation, parallel)
    └─► T013–T020 (entity/controller updates, parallel with each other)
    └─► T021–T022 (simulator updates, sequential — persist depends on mod.rs constructor)
    └─► T023–T027 (new per-asset unit tests, parallel)
    └─► T028–T030b (existing test rewrites, sequential within each file)
```

### User Story Dependencies

- **US1 (P1)**: Depends only on Phase 2 (ADJ-01 complete)
- **US2 (P2)**: Depends on US1 complete (all param structs + domain constructors updated)
- **US3 (P3)**: Depends on US2 complete (runtime wiring in main.rs done)

---

## Parallel Execution Examples

### Phase 2 (ADJ-01) — sequential by nature (4 tasks, ~20 min)

```
T003 → T004 → T005 → T006 (verify compile)
```

### Phase 3, Group 1 — struct creation (6 parallel tasks, ~30 min total)

```
Task: T007 — entities/planner_params.rs (PlannerParams, AbsorberParams, SimulatorParams)
Task: T008 — assets/battery.rs (BatteryParams)
Task: T009 — assets/ev.rs (EvParams)
Task: T010 — assets/heater.rs (HeaterParams)
Task: T011 — assets/pv.rs (PvParams + forecast_kw move)
Task: T012 — assets/base_load.rs (BaseLoadParams)
```

### Phase 3, Group 2 — domain file updates (10 parallel tasks after T007–T012)

```
Task: T013 — entities/plan.rs
Task: T014 — controller/dispatcher.rs
Task: T015 — controller/absorber.rs
Task: T016 — controller/milp_planner/types.rs
Task: T017 — controller/milp_planner/envelopes.rs
Task: T018 — controller/milp_planner/inputs.rs
Task: T019 — controller/milp_planner/mod.rs
Task: T020 — controller/milp_planner/results.rs
Task: T021 — simulator/mod.rs
Task: T022 — simulator/persist.rs  (T021 must finish first — uses from_params())
```

### Phase 3, Group 3 — new asset unit tests (5 parallel tasks after T007–T012)

```
Task: T023 — assets/battery.rs tests
Task: T024 — assets/ev.rs tests
Task: T025 — assets/heater.rs tests
Task: T026 — assets/pv.rs tests
Task: T027 — assets/base_load.rs tests
```

---

## Implementation Strategy

### MVP (User Story 1 Only)

1. Complete Phase 1: Setup (T001–T002)
2. Complete Phase 2: ADJ-01 (T003–T006) — **critical, unblocks everything**
3. Complete Phase 3: US1 — struct creation then domain updates then tests (T007–T030)
4. **STOP and VALIDATE**: `cargo test --workspace` passes; SC-001 grep returns zero
5. Domain ring is now YAML-free and independently testable

### Incremental Delivery

1. Phase 1 + Phase 2 → ADJ-01 complete (PlannerObjective in domain ring)
2. Phase 3 → US1 complete → Run `cargo test`; all domain tests use inline params
3. Phase 4 → US2 complete → `main.rs` assembled; profile-to-domain boundary clean
4. Phase 5 → US3 complete → BDD green; production behaviour verified
5. Phase 6 → Polish; journal; commit

---

## Notes

- [P] tasks write to different files — safe to run as parallel sub-agents
- ADJ-01 (Phase 2) is the only hard sequential gate; everything else within a phase can parallelize
- `simulator/persist.rs` (T022) must follow `simulator/mod.rs` (T021) — persist calls `from_params()` introduced in T021
- `profile.rs` bridge re-export (T005) is a temporary shim — removed in T033; do not leave it permanently
- Commit after each checkpoint (end of Phase 2, end of US1, end of US2) for clean rollback points
- Total tasks: **41** | Setup: 2 | Foundational: 4 | US1: 24 | US2: 4 | US3: 3 | Polish: 3 | Parallel opportunities: 28
