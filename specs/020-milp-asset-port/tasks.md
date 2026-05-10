# Tasks: MILP Asset Port — Decouple Planner from Concrete Asset Types

**Feature**: `020-milp-asset-port` | **Branch**: `020-milp-asset-port`  
**Input**: `specs/020-milp-asset-port/` (plan.md, spec.md, data-model.md, research.md, contracts/asset_milp_context.md)

**Tests**: Unit tests are included (spec mandates new test surface per Constitution VI and US2 requirement).

**Organization**: Tasks group by user story to enable independent implementation and testing. Phases 3–5 each represent a complete, independently verifiable increment.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no shared state dependencies)
- **[Story]**: Which user story this task belongs to (US1, US2, US3)
- Exact file paths are included in all descriptions

---

## Phase 1: Setup (Baseline Verification)

**Purpose**: Confirm the branch compiles cleanly before any changes. Establishes a regression baseline.

- [ ] T001 Run `cargo check` in `VEN/` on the current branch and confirm zero errors — this is the compilation baseline before Phase 2 changes begin

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Define the `AssetMilpContext` port and relocate `*MilpVars` types from `assets/` to `controller/`. All user story phases depend on this phase completing successfully.

**⚠️ CRITICAL**: No user story work can begin until T005 passes `cargo check` cleanly.

- [ ] T002 Create `VEN/src/controller/milp/asset_port.rs` — define `AssetKind` enum (`Battery`, `Ev`, `Heater`), `BatteryScalars` / `EvScalars` / `HeaterScalars` parameter structs, `AssetMilpParams` enum (variants: `Battery(BatteryScalars)`, `Ev(EvScalars)`, `Heater(HeaterScalars)`, `Unknown`), and `AssetMilpContext` trait with 6 methods (`asset_id`, `asset_kind`, `milp_params`, `declare_vars_into_pool`, `constraints`, `objective`) per `specs/020-milp-asset-port/contracts/asset_milp_context.md` and `specs/020-milp-asset-port/data-model.md`

- [ ] T003 Register `pub mod asset_port` in `VEN/src/controller/milp/mod.rs`; add `pub use asset_port::{AssetKind, AssetMilpContext, AssetMilpParams, BatteryScalars, EvScalars, HeaterScalars}` re-exports; remove the existing `#[allow(unused_imports)]` block that re-exports `Battery`, `EvCharger`, `Heater`, `PvInverter` from `crate::assets` (these were only used by test submodules via `use super::*`, and test mocks will replace them in Phase 4)

- [ ] T004 Move `BatteryMilpVars`, `EvMilpVars`, `HeaterMilpVars` struct definitions from `VEN/src/assets/battery.rs`, `VEN/src/assets/ev.rs`, `VEN/src/assets/heater.rs` into `VEN/src/controller/milp_interactions.rs` (place them before the `MilpVarPool` definition); remove the three `use crate::assets::{battery::BatteryMilpVars, ev::EvMilpVars, heater::HeaterMilpVars}` import lines from `milp_interactions.rs`; add `use crate::controller::milp_interactions::{BatteryMilpVars, EvMilpVars, HeaterMilpVars}` back to each of the three asset files so their existing `declare_vars()` methods still compile

- [ ] T005 Run `cargo check` in `VEN/` and fix all compilation errors from the `*MilpVars` relocation — expected fixes: update `VEN/src/controller/milp/solver_phase1.rs` and `VEN/src/controller/milp/solver_phase2.rs` to import `BatteryMilpVars`, `EvMilpVars`, `HeaterMilpVars` from `crate::controller::milp_interactions` instead of `crate::assets::*` (these files still temporarily retain `BatteryMilpContext` etc. imports — those are removed in Phase 3)

**Checkpoint**: `cargo check` green → user story phases can begin (US1, US2 in parallel where noted)

---

## Phase 3: User Story 1 — Extensibility Port (Priority: P1) 🎯 MVP

**Goal**: Any developer can add a new asset type by implementing `AssetMilpContext` only. Neither `solver_phase1.rs`, `solver_phase2.rs`, nor `milp_interactions.rs` require modification. Constitution invariant holds: `grep -r "use crate::assets::" VEN/src/controller/milp` returns empty.

**Independent Test**: `grep -r "use crate::assets::" VEN/src/controller/milp` → empty; `cargo check` green; `cargo test` all existing tests still pass.

### Implementation for User Story 1

- [ ] T006 [P] [US1] Implement `AssetMilpContext` for `BatteryMilpContext` in `VEN/src/assets/battery.rs`: `asset_id()` → `"battery"`, `asset_kind()` → `AssetKind::Battery`, `milp_params(n, step_s, now)` → `AssetMilpParams::Battery(BatteryScalars { e_nom_kwh: self.e_nom_kwh, e_init_kwh: self.e_init_kwh, e_min_kwh: self.e_min_kwh, e_max_kwh: self.e_max_kwh, p_ch_max_kw: self.p_ch_max_kw, p_dis_max_kw: self.p_dis_max_kw, eff_ch: self.eff_ch, eff_dis: self.eff_dis })`, `declare_vars_into_pool()` calls `self.declare_vars(n, c_startup_eur, c_ramp_eur_kw, vars)` and assigns result to `pool.bat = Some(...)`, `constraints()` delegates to `self.constraints(pool.bat.as_ref().unwrap(), n, dt_h)`, `objective()` delegates to `BatteryMilpContext::objective(pool.bat.as_ref().unwrap(), c_wear_eur_kwh, c_startup_eur, c_ramp_eur_kw, n, dt_h)`

- [ ] T007 [P] [US1] Implement `AssetMilpContext` for `EvMilpContext` in `VEN/src/assets/ev.rs`: `asset_id()` → `"ev"`, `asset_kind()` → `AssetKind::Ev`, `milp_params(n, step_s, now)` → `AssetMilpParams::Ev(EvScalars { mode: translate EvMilpMode→MilpLoadMode (same logic as already in solver_phase1.rs lines 46-50), a_ev: self.a_ev.clone(), t_dead_step: self.t_dead_step, p_max_kw: self.p_max_kw, p_min_kw: self.p_min_kw, e_core_kwh: self.e_core_kwh, e_extra_max_kwh: self.e_extra_max_kwh, v_extra_eur_kwh: self.v_extra_eur_kwh })`, `declare_vars_into_pool()` wraps existing EV var-declaration logic and sets `pool.ev`, `constraints()` and `objective()` delegate to existing EvMilpContext methods

- [ ] T008 [P] [US1] Implement `AssetMilpContext` for `HeaterMilpContext` in `VEN/src/assets/heater.rs`: `asset_id()` → `"heater"`, `asset_kind()` → `AssetKind::Heater`, `milp_params(n, step_s, now)` → `AssetMilpParams::Heater(HeaterScalars { mode: translate HeaterMilpMode→MilpLoadMode, t_dead_step: self.t_dead_step, p_mid_kw: self.p_mid_kw, p_full_kw: self.p_full_kw, e_init_kwh: self.e_init_kwh, e_max_kwh: self.e_max_kwh, q_dem_kw: self.q_dem_kw.clone(), e_target_kwh: self.e_target_kwh, lambda_sw_eur: self.lambda_sw_eur })`; fill in any existing `todo!()` stubs in heater MILP methods; `declare_vars_into_pool()`, `constraints()`, `objective()` delegate to existing `HeaterMilpContext` methods

- [ ] T009 [US1] Rewrite `build_milp_inputs()` in `VEN/src/controller/milp/inputs.rs` — add `asset_contexts: &[Box<dyn AssetMilpContext>]` as the first parameter; remove the three `if let Some(cfg) = profile.battery_config()` / `profile.ev_config()` / `profile.heater_config()` blocks and the `SimSnapshot` `assets` parameter (grid-only fields still read from `profile`); replace with `for ctx in asset_contexts { match ctx.milp_params(n, step_s, now) { AssetMilpParams::Battery(b) => { /* assign MilpInputs battery scalar fields from b */ }, AssetMilpParams::Ev(e) => { /* assign MilpInputs EV scalar fields from e */ }, AssetMilpParams::Heater(h) => { /* assign MilpInputs heater scalar fields from h */ }, AssetMilpParams::Unknown => {} } }`; remove all `use crate::assets::*` imports from `inputs.rs`

- [ ] T010 [P] [US1] Rewrite `solve_phase1()` in `VEN/src/controller/milp/solver_phase1.rs` — add `asset_contexts: &[Box<dyn AssetMilpContext>]` parameter; remove the `bat_ctx` / `ev_ctx` / `heat_ctx` local variable blocks that reconstruct `*MilpContext` from `MilpInputs` scalar fields (lines ~33-88); replace with `for ctx in asset_contexts { ctx.declare_vars_into_pool(n, 0.0, 0.0, vars, &mut pool) }` (Phase 1: startup and ramp penalties are 0.0); replace per-asset `cs.extend(bat_ctx.constraints(...))` blocks with `for ctx in asset_contexts { cs.extend(ctx.constraints(&pool, n, dt_h)) }`; replace per-asset objective additions with `for ctx in asset_contexts { obj += ctx.objective(&pool, n, dt_h, p1w.c_bat_wear_eur_kwh, 0.0, 0.0) }`; remove `use crate::assets::*` imports; grid-balance constraint and `build_interactions()` call remain unchanged. *(Note on heater switching cost: `HeaterMilpContext::objective()` reads `self.lambda_sw_eur` from its own field — Phase 1 passes `c_startup_eur = 0.0` to the generic trait method but the heater switching coefficient is asset-internal; verify the heater `objective()` implementation reads `self.lambda_sw_eur` not the generic parameter — M5)*

- [ ] T011 [P] [US1] Rewrite `solve_phase2()` in `VEN/src/controller/milp/solver_phase2.rs` — same pattern as T010; Phase 2 passes actual `p2w.c_bat_startup_eur` and `p2w.c_bat_ramp_eur_kw` to `ctx.objective()` instead of 0.0; `declare_vars_into_pool()` passes `p2w.c_bat_startup_eur` and `p2w.c_bat_ramp_eur_kw` (so battery binary startup/ramp vars are allocated); remove `use crate::assets::*` imports. *(Same heater note as T010: `HeaterMilpContext::objective()` reads `self.lambda_sw_eur` from its own context — Phase 2 passes actual `c_bat_startup_eur` / `c_bat_ramp_eur_kw` for battery but the heater coefficient is self-contained)*

- [ ] T012 [US1] Update `run_planner()` (or `solve_milp_two_phase()`) signature in `VEN/src/controller/milp/mod.rs` — add `asset_contexts: Vec<Box<dyn AssetMilpContext>>` parameter and thread `&asset_contexts` into `build_milp_inputs()`, `solve_phase1()`, `solve_phase2()`; update the `run_planner()` call site in `VEN/src/tasks/planning.rs` to build `asset_contexts` before calling the planner by iterating profile asset configs and calling `cfg.build_milp_context(snap, ev_session, heater_target, n, step_s, now)` for each configured asset type; **add a `debug_assert!` at the top of `run_planner()`** that verifies no two entries in `asset_contexts` share the same `asset_kind()` value — `MilpVarPool` has exactly one named slot per kind and silently overwrites on duplicate; the assert fires in debug builds only and documents the single-per-kind assumption (Phase 3 limitation; multi-instance support requires a Vec-based pool redesign in a future phase)

- [ ] T013 [US1] Update `AssetConfig::build_milp_context()` in `VEN/src/assets/mod.rs` — change the return type from `Option<AnyMilpContext>` to `Option<Box<dyn AssetMilpContext>>`; wrap the existing construction results in `Box::new(...)` calls; retain `AnyMilpContext` as a `pub(crate)` (or private) internal construction helper so no construction logic is rewritten; add `use crate::controller::milp::AssetMilpContext` import to `assets/mod.rs`; fix any call sites that pattern-match on `AnyMilpContext` variants (should only be in `assets/mod.rs` itself after this change); **ensure the function signature includes `ev_session: Option<&EvSession>` and `heater_target: Option<&HeaterTarget>` parameters** — these were previously consumed by `build_milp_inputs()` but must now be wired through the context-construction path so `EvMilpContext::from_state()` and `HeaterMilpContext` construction receive the session/target data (T012 confirms the call site in `tasks/planning.rs` passes them)

- [ ] T014 [US1] Run `cargo check` in `VEN/` — confirm clean compilation; run `grep -r "use crate::assets::" VEN/src/controller/milp` and confirm the output is empty (Constitution Principle VI invariant); run `grep -n "crate::assets::battery::Battery\b\|crate::assets::ev::EvCharger\b\|crate::assets::heater::Heater\b" VEN/src/controller/milp_interactions.rs` and confirm empty output; run `grep -rn "impl AssetMilpContext" VEN/src/assets/` and confirm only `BatteryMilpContext`, `EvMilpContext`, `HeaterMilpContext` appear — no `GridAsset`, `PvInverter`, or base-load type should implement the trait (FR-004, FR-005)

**Checkpoint**: Constitution invariant passes; compilation clean → User Story 1 (extensibility) is complete. **T015–T017 (US2 per-asset tests) MAY begin now in parallel** — they depend only on T006–T008 (asset impls), which complete within this phase, not on T012–T014.

---

## Phase 4: User Story 2 — Isolatable Unit Tests (Priority: P2)

**Goal**: Each concrete `AssetMilpContext` implementation (`BatteryMilpContext`, `EvMilpContext`, `HeaterMilpContext`) can be tested independently without running the full two-phase MILP solver. Planner unit tests use lightweight mock contexts with no `crate::assets::` imports.

**Independent Test**: `cargo test` in `VEN/` passes; new tests in `assets/battery.rs`, `assets/ev.rs`, `assets/heater.rs`, and `controller/milp/tests/` all green.

### Implementation for User Story 2

- [ ] T015 [P] [US2] Add `#[cfg(test)]` block to `VEN/src/assets/battery.rs` — write unit tests for `BatteryMilpContext`'s `AssetMilpContext` implementation: (1) `milp_params()` returns `AssetMilpParams::Battery` with all scalar fields matching the context's own fields; (2) `declare_vars_into_pool()` sets `pool.bat = Some(...)` and the `BatteryMilpVars` vector lengths equal `n`; (3) `constraints()` returns non-empty `Vec<Constraint>` and count matches expected (4n + ramp constraints); no solver invocation needed — construct a `ProblemVariables` inline, call `declare_vars_into_pool()`, then call `constraints()`

- [ ] T016 [P] [US2] Add `#[cfg(test)]` block to `VEN/src/assets/ev.rs` — write unit tests for `EvMilpContext`'s `AssetMilpContext` implementation: (1) `milp_params()` for `MustRun` / `MayRun` / `MustNotRun` modes each return correct `EvScalars.mode`; (2) `milp_params()` propagates `a_ev` availability mask unchanged; (3) `declare_vars_into_pool()` sets `pool.ev = Some(...)` with correct vector lengths; use `n = 4` for test speed

- [ ] T017 [P] [US2] Add `#[cfg(test)]` block to `VEN/src/assets/heater.rs` — write unit tests for `HeaterMilpContext`'s `AssetMilpContext` implementation: (1) `milp_params()` returns `AssetMilpParams::Heater` with `q_dem_kw` vec length == `n`; (2) `declare_vars_into_pool()` sets `pool.heater = Some(...)` with correct vector lengths; (3) fill any existing `todo!()` stubs in heater MILP constraint/objective methods that were previously skipped

- [ ] T018 [US2] Create `VEN/src/services/test_support/milp_mocks.rs` — define `MockBatteryCtx`, `MockEvCtx`, `MockHeaterCtx` test-double structs compiled in **all** builds (no `#[cfg(test)]`) that implement `AssetMilpContext` without importing from `crate::assets::*` (use `BatteryMilpVars` / `EvMilpVars` / `HeaterMilpVars` from `crate::controller::milp_interactions`); register with `pub mod milp_mocks;` in `VEN/src/services/test_support/mod.rs`; update any existing planner unit tests in `controller/milp/tests/` that previously used `MilpInputs`-only construction to additionally pass `&[Box<dyn AssetMilpContext>]` (use mock contexts from `crate::services::test_support::milp_mocks`) for the new solver signatures. *(Constitution Principle VI: mock adapters live in `VEN/src/services/test_support/`, not in `#[cfg(test)]` scope)*

**Checkpoint**: All per-asset trait implementations tested in isolation → User Story 2 complete

---

## Phase 5: User Story 3 — Behaviour Unchanged (Priority: P3)

**Goal**: End-to-end two-phase MILP solve with all three asset types produces per-slot results within absolute difference ≤ 1 × 10⁻⁶ kW of the pre-refactoring baseline for n=24 (SC-005). A new n=48 (24h horizon, 1800s steps) regression test covers the full PV irradiance cycle with battery + EV + heater + PV.

**Independent Test**: `cargo test` in `VEN/` green; `solve_milp_two_phase()` output for n=24 stays within 5 % of baseline; n=48 test completes in < 5 s with physically valid output; all 232 BDD scenarios pass on Pi4-Server (T026).

### Implementation for User Story 3

- [ ] T019 [US3] Create the n=48 test profile fixture — either as `VEN/src/controller/milp/tests/profiles/test48.yaml` (YAML file, loaded in tests via `include_str!()`) or as a `const`/`fn` inline in `VEN/src/controller/milp/tests/planner.rs`; profile parameters per `specs/020-milp-asset-port/data-model.md` §"Test Profile — n=48": `plan_horizon_h: 24`, `plan_step_s: 1800`, battery `e_nom_kwh: 10.0` / `p_ch_max_kw: 5.0` / `p_dis_max_kw: 5.0` / `e_min_kwh: 1.0` (10 % DoD floor) / `e_init_kwh: 5.0`, EV 40 kWh / 7.2 kW (must-run, 50 % initial SoC), heater 2 kW full / 1 kW mid, PV 6 kWp

- [ ] T020 [US3] Add n=48 regression test in `VEN/src/controller/milp/tests/planner.rs` — load the test48 profile, build `Vec<Box<dyn AssetMilpContext>>` from mock contexts (T018) or by calling `AssetConfig::build_milp_context()` with synthetic snapshot; call `solve_milp_two_phase()` (the full two-phase orchestrator); assert: `SolveOutput` fields are all finite; battery SoC trajectory ∈ [`e_min_kwh / e_nom_kwh` = 0.1, 1.0] throughout all 48 slots (derive from T019 fixture values); net grid import ≤ `max_import_kw` per slot; `SolveOutput::net_cost_eur` is finite; per-slot deviation from the n=24 pre-Phase-3 baseline does not exceed 1 × 10⁻⁶ kW absolute (SC-005); test must complete in < 5 s. **Also add edge-case assertions**: (a) call `solve_milp_two_phase()` with an **empty** `asset_contexts` slice and assert the result is a valid `SolveOutput` (grid-only plan, no panic); (b) call with an `EvMilpContext` in `MustNotRun` mode and assert the EV does not appear in the plan's controllable-asset allocation; **(c) two-same-type guard (C1)**: `MilpVarPool` has one named slot per asset kind — passing two `MockBatteryCtx` instances is unsupported and must be rejected; verify that the `debug_assert!` added to `run_planner()` in T012 fires in debug builds when duplicate `asset_kind()` values are detected in the slice; **(d) solver infeasibility (C2)**: construct a `MockBatteryCtx` whose `constraints()` returns a contradictory bound (e.g. force `e_min > e_max` by returning a trivially infeasible constraint); call `solve_milp_two_phase()` and assert the result is a valid, non-panicking `SolveOutput` that matches the pre-Phase-3 fallback behaviour (solver returns `Err` / fallback plan, not a panic)

- [ ] T021 [US3] Run `cargo test` in `VEN/` — all tests (including existing n=24 baseline tests in `tests/planner.rs`, heater tests in `tests/heater.rs`, and all new tests from T015–T020) must pass; fix any remaining test failures caused by the new `asset_contexts` parameter in solver/inputs function signatures (add mock contexts where needed)

- [ ] T026 [US3] SSH to Pi4-Server and run the full BDD suite against the Phase 3 VEN image: `docker compose -f tests/docker-compose.test.yml run --build --rm test-runner` from `/srv/docker/openadr_lab/`; assert all 232 scenarios pass with zero failures; use `--build` to ensure the latest VEN image is built (SC-002, FR-007). *(This is the runtime regression gate — `cargo test` alone does not cover the full BDD scenario suite)*

**Checkpoint**: Full test suite green; n=48 regression passing; all 232 BDD scenarios passing (T026) → User Story 3 complete, Phase 3 of the overall refactoring is done

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Documentation, final invariant checks, and cleanup.

- [ ] T022 [P] Update module doc comment at top of `VEN/src/controller/milp_interactions.rs` to note that `BatteryMilpVars`, `EvMilpVars`, `HeaterMilpVars` are now defined in this file (relocated from `assets/`) and that `milp_interactions.rs` no longer imports from `crate::assets`

- [ ] T023 Run final constitution invariant verification: `grep -r "use crate::assets::" VEN/src/controller/milp` → must return empty (no matches); `grep -n "crate::assets::battery::Battery\b\|crate::assets::ev::EvCharger\b\|crate::assets::heater::Heater\b" VEN/src/controller/milp_interactions.rs` → must return empty; `grep -rn "impl AssetMilpContext" VEN/src/assets/` → must list only `BatteryMilpContext`, `EvMilpContext`, `HeaterMilpContext` (no `GridAsset`, `PvInverter`, or base-load type — FR-004, FR-005); document results in a comment at the top of `asset_port.rs`; **file-size check (Constitution VI)**: `(Get-Content VEN/src/controller/milp/asset_port.rs).Count` → must be ≤ 500; `(Get-Content VEN/src/controller/milp_interactions.rs).Count` → must be ≤ 500; `(Get-Content VEN/src/assets/battery.rs).Count` → must be ≤ 500; `(Get-Content VEN/src/assets/ev.rs).Count` → must be ≤ 500; `(Get-Content VEN/src/assets/heater.rs).Count` → must be ≤ 500; fail the phase if any file exceeds the limit

- [ ] T024 Run `cargo test` in `VEN/` one final time — all tests green before committing

- [ ] T025 [P] Update `docs/plans/ven_backend_architecture_refactoring.md` — mark Phase 3 as ✅ complete; record the constitution invariant grep result as verification evidence; update the AB-02 breach status to "resolved"

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Setup)**: No dependencies — start immediately
- **Phase 2 (Foundational)**: Depends on Phase 1 — **BLOCKS all user story phases**
- **Phase 3 (US1)**: Depends on Phase 2 (T005 clean)
- **Phase 4 (US2)**: T015–T017 depend only on T002 (trait defined) + T006–T008 (asset impls) — can overlap with Phase 3 mid-execution; T018 depends on T002 + T012
- **Phase 5 (US3)**: Depends on Phase 3 (T014) and Phase 4 (T018); T026 (BDD) depends on T021
- **Phase 6 (Polish)**: Depends on Phase 5

### User Story Dependencies

- **US1 (P1)**: Can begin after Phase 2 — no dependency on US2 or US3
- **US2 (P2)**: T015–T017 can start alongside US1 once T006–T008 are done; T018 can start once T012 is done
- **US3 (P3)**: Depends on US1 (T014) and US2 (T018 for mocks)

### Within Phase 3 (US1)

```
T006 ──┐
T007 ──┼──→ T009 ──→ T012 ──→ T013 ──→ T014
T008 ──┘
        └──→ T010 ──→ T012
        └──→ T011 ──→ T012
```

### Parallel Opportunities

- T006, T007, T008 (asset trait impls — different files, no deps)
- T010, T011 (solver_phase1.rs, solver_phase2.rs — different files)
- T015, T016, T017 (asset unit tests — different files)
- T022, T025 (docs — different files)

---

## Parallel Example: Phase 3 (User Story 1)

```
# Once T002–T005 complete, launch T006, T007, T008 together:
Task T006: "Implement AssetMilpContext for BatteryMilpContext in VEN/src/assets/battery.rs"
Task T007: "Implement AssetMilpContext for EvMilpContext in VEN/src/assets/ev.rs"
Task T008: "Implement AssetMilpContext for HeaterMilpContext in VEN/src/assets/heater.rs"

# Once T006–T008 complete, launch T010 and T011 together:
Task T010: "Rewrite solve_phase1() in VEN/src/controller/milp/solver_phase1.rs"
Task T011: "Rewrite solve_phase2() in VEN/src/controller/milp/solver_phase2.rs"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Baseline verification (T001)
2. Complete Phase 2: Foundational — trait + type relocation (T002–T005)
3. Complete Phase 3: US1 — implement port, rewrite planner entry points (T006–T014)
4. **STOP and VALIDATE**: Constitution invariant holds; `cargo check` green
5. US1 is the full MVP — the planner now accepts `Vec<Box<dyn AssetMilpContext>>`

### Incremental Delivery

1. Phase 1 + 2 → Trait and types defined, compilation clean
2. Phase 3 → Port wired end to end → US1 validated (MVP)
3. Phase 4 → Per-asset tests and mock infrastructure → US2 validated
4. Phase 5 → n=24 regression passes, n=48 new baseline added → US3 validated
5. Phase 6 → Clean, documented, archived

---

## Notes

- [P] tasks = different files, no shared-state dependencies, safe to run concurrently
- [Story] label maps each task to a user story for traceability to spec.md
- `MilpInputs` struct in `types.rs` is deliberately **not changed** — existing unit tests that construct it directly continue to compile without modification
- `AnyMilpContext` enum is deliberately **retained** as `pub(crate)` in `assets/mod.rs` — only its public return type changes
- Tests in `controller/milp/tests/` use mock `AssetMilpContext` implementations (T018) — they never import from `crate::assets::` (required to pass constitution invariant)
- Each phase checkpoint is independently verifiable with `cargo check` / `cargo test`
- Total tasks: **26** (T001–T026)
