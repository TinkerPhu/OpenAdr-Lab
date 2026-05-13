# Tasks: Deterministic Test Environment for MILP-Backed BDD Tests

**Branch**: `022-deterministic-test-env`
**Input**: Design documents from `specs/022-deterministic-test-env/`
**Spec**: [spec.md](./spec.md) | **Plan**: [plan.md](./plan.md) | **Data Model**: [data-model.md](./data-model.md)

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: Which user story this task belongs to (US1, US2, US3)

---

## Phase 1: Foundational (Blocking Prerequisites)

**Purpose**: Extend `SimInjectState` and the inject endpoint with the new `pv_plan_kw` field. These two files must compile before ANY planner or BDD work can begin.

**⚠️ BDD-First gate (Constitution Prin II)**: T000 MUST be committed before any Phase 1 Rust code is written. The red test for US1 already exists (`deviation_absorber.feature:149`, tagged `@wip`). T000 creates the red test for US3.

**⚠️ CRITICAL**: No user story implementation can begin until T000+T003 are both complete.

- [x] T000 [BDD-First / US3] Write a `@wip` stub scenario in `tests/features/ven_planner.feature` BEFORE writing any Phase 1 Rust code — satisfies Constitution Prin II for US3: `Scenario: PV forecast override does not trigger a replan` with step skeleton `When I set pv plan forecast to 0.0 kW` / `Then no plan cycle is triggered within 2 seconds`. Tag it `@wip`. Step definitions need not exist yet. Commit the stub so the red test pre-dates all implementation (T019+T021 will make it green).
- [x] T001 Add `pub pv_plan_kw: Option<f64>` to `SimInjectState` struct and `pv_plan_kw: None` to its `Default` impl in `VEN/src/state.rs`
- [x] T002 [P] Add `pub pv_plan_kw: Option<serde_json::Value>` with `#[serde(default)]` to `PostSimInjectBody` in `VEN/src/routes/sim.rs`; add `merge_f64!(pv_plan_kw)` in `merge_inject`; do NOT add `pv_plan_kw` to the `should_replan` guard *(note: [P] means write concurrently with T001; compile only after both T001+T002 edits are applied — T003 performs the build check)*
- [x] T003 Run `SQLX_OFFLINE=true cargo build` from `VEN/` to confirm both struct changes compile cleanly

**Checkpoint**: `SimInjectState` carries `pv_plan_kw`; `POST /sim/inject` accepts and merges it; setting `pv_plan_kw` does not trigger a replan. All downstream work can now proceed.

---

## Phase 2: User Story 1 — Freeze PV Forecast (Priority: P1) 🎯 MVP

**Goal**: Wire `pv_plan_kw` through the planner call chain so every MILP solve uses the constant forecast when set; add the BDD step and unblock `deviation_absorber.feature:149`.

**Independent Test**: Run `deviation_absorber.feature:149` at three different times of day (morning, afternoon, evening) on Pi4-Server — all three must pass without the `@wip` tag.

### Implementation for User Story 1

- [x] T004 [P] [US1] Add `pv_forecast_override: Option<f64>` parameter to `run_planner` in `VEN/src/controller/milp_planner/mod.rs`; forward it as the last argument in the `build_milp_inputs(...)` call
- [x] T005 [P] [US1] Add `pv_forecast_override: Option<f64>` parameter to `build_milp_inputs` in `VEN/src/controller/milp_planner/inputs.rs`; in the per-slot PV `for t in 0..n` loop, check the override first: `if let Some(forced_kw) = pv_forecast_override { p_pv.push(forced_kw.max(0.0)); } else { /* existing natural + decayed_offset logic unchanged */ }`
- [x] T006 [P] [US1] Add `@given("I set pv plan forecast to {kw:f} kW")` step in `tests/features/steps/phase_a_physics_steps.py` that POSTs `{"pv_plan_kw": kw}` to `/sim/inject` and asserts HTTP 204
- [x] T007 [US1] In `VEN/src/tasks/planning.rs`, after `let inject_snap = state.inject_state().await;`, capture `let pv_forecast_override = inject_snap.pv_plan_kw;`; pass it into `run_planner(...)` inside the `spawn_blocking` closure as the new `pv_forecast_override` argument (note: the closure already `move`s all locals, so `pv_forecast_override` is captured automatically)
- [x] T008 [US1] In `tests/features/deviation_absorber.feature`, add `And I set pv plan forecast to 0.0 kW` to the Background (after `And I inject pv irradiance 0.0 via sim inject`); remove the `@wip` tag from the scenario beginning at line 149 (`DeviceDeviation does not fire for transient deviations`)

### Verification for User Story 1

- [x] T026 [US1] Write a cargo unit test for `build_milp_inputs` (add to its `#[cfg(test)]` block in `VEN/src/controller/milp_planner/inputs.rs` or a neighbouring test module): **(a)** call with `pv_forecast_override=Some(0.0)`, `n=24` — assert all 24 `p_pv` slots equal `0.0`; **(b)** call again immediately — assert output is bitwise-identical (covers **US1-AC-2**: race-condition second solve produces identical plan); **(c)** call with `pv_forecast_override=None` — assert slots are non-zero (natural model active). Depends on T005 (function signature must exist first).
- [x] T009 [US1] Run `SQLX_OFFLINE=true cargo test --workspace` from `VEN/` — expect zero failures; T026 unit test is included in this run
- [x] T010 [US1] On Pi4-Server: run `docker compose -f tests/docker-compose.test.yml run --build --rm test-runner features/deviation_absorber.feature` — confirm the previously-`@wip` scenario now passes and all other deviation-absorber scenarios remain passing

**Checkpoint**: `pv_plan_kw` override active end-to-end; `deviation_absorber.feature:149` passes at any time of day; cargo tests clean.

---

## Phase 3: User Story 2 — Suite-Wide Adoption (Priority: P2)

**Goal**: Audit every MILP-backed BDD feature file that makes battery-dispatch assertions and add `pv_plan_kw=0.0` (or matching value) to its Background where time-of-day sensitivity could cause non-determinism. Apply at Background level when all scenarios in the file exercise MILP battery dispatch; apply per-scenario only when some scenarios intentionally test time-varying dispatch.

**Independent Test**: Run the full BDD suite on Pi4-Server after all updates; confirm zero regressions vs the pre-change baseline.

### Implementation for User Story 2

- [x] T011 [P] [US2] Audit `tests/features/ven_dispatcher.feature` — has `battery power_kw < -1.0` assertion (line 51); added `And I set pv plan forecast to 0.0 kW` to Background
- [x] T012 [P] [US2] Audit `tests/features/use_cases.feature` — UC6 checks event/report payload structure only (no battery power-level assertions); pv_plan_kw not needed
- [x] T013 [P] [US2] Audit `tests/features/ven_planner.feature` — no direct battery power assertions; added `And I set pv plan forecast to 0.0 kW` to Background for defensive determinism
- [x] T014 [P] [US2] Audit `tests/features/ven_uc_normal.feature` — UC-03 uses per-scenario pv_irradiance override; added `And I set pv plan forecast to 0.0 kW` to Background for all other scenarios
- [x] T015 [P] [US2] Audit `tests/features/ven_uc_stress.feature` — UC-12c uses per-scenario pv_irradiance override; added `And I set pv plan forecast to 0.0 kW` to Background for all other scenarios
- [x] T016 [P] [US2] Audit `tests/features/asset_forecast.feature` — tests forecast API structure only; no battery dispatch headroom assertions; pv_plan_kw not needed
- [x] T017 [P] [US2] Audit `tests/features/ven_timeline.feature` — tests timeline API structure only; no battery dispatch headroom assertions; pv_plan_kw not needed
- [x] T018 [US2] On Pi4-Server: run the full BDD suite (`bash run_all_tests.sh --e2e` or equivalent) with `--build` flag; confirm zero regressions across all previously-passing scenarios

**Checkpoint**: All seven MILP-backed BDD feature files are audited; `pv_plan_kw` applied wherever battery dispatch headroom is asserted; full suite green.

---

## Phase 4: User Story 3 — No Replan on Inject (Priority: P3)

**Goal**: Explicitly verify that setting `pv_plan_kw` via `POST /sim/inject` does not trigger an immediate MILP replan — protecting the assertion window in timing-sensitive BDD steps.

**Independent Test**: A BDD scenario injects `pv_plan_kw`, waits 2 seconds, and asserts no `planner loop: starting plan cycle` log entry appeared (exact string confirmed at `VEN/src/tasks/planning.rs:52`).

### Implementation for User Story 3

- [x] T019 [US3] Remove the `@wip` tag from the stub scenario added in T000; complete the scenario body in `tests/features/ven_planner.feature` covering US3 AC-1: `Given the system is idle, When I set pv plan forecast to 0.0 kW, Then no plan cycle is triggered within 2 seconds`; add a corresponding step definition `@then("no plan cycle is triggered within {sec:d} seconds")` in `tests/features/steps/phase_a_physics_steps.py` that calls `GET /plan` (or equivalent plan-status endpoint) via `ctx.ven_api` every 200 ms for `sec` seconds and asserts the plan's solve timestamp does not advance — confirming no `planner loop: starting plan cycle` event fired (note: asserting `AssetStateChange` absence would be incorrect; the replan indicator is the log string at `tasks/planning.rs:52` which correlates with a plan-timestamp change) *(US3 AC-2 — "tariff event fires → solve uses override" — is covered implicitly by Phase 3 suite-wide adoption)*
- [x] T020 [US3] Code review: open `VEN/src/routes/sim.rs` and confirm `pv_plan_kw` is absent from the `should_replan` boolean expression (lines around `let should_replan = body.pv_irradiance.is_some() || ...`); add a clarifying inline comment in `merge_inject` if the no-replan exclusion is not already evident
- [x] T021 [US3] On Pi4-Server: run the new US3 BDD scenario (T019) with `--build` flag — confirm it passes; cross-check by running with `pv_irradiance` inject instead (which triggers a plan cycle, confirming the contrast)

**Checkpoint**: US3 BDD scenario green on Pi4; code-review confirmation documented; no-replan contract verifiable and tested via BDD.

---

## Phase 5: Polish & Cross-Cutting Concerns

**Purpose**: Final validation, documentation, and commit hygiene.

- [x] T022 [P] Run the full quickstart.md acceptance checklist (`specs/022-deterministic-test-env/quickstart.md`) — all 7 verification steps pass; quickstart Step 5 (`cargo test`) can be skipped if T009 already passed cleanly in the same build
- [x] T023 [P] Verify architecture boundary: **(a)** `grep -r "pv_plan_kw" VEN/src/` finds exactly 3 files (`state.rs`, `routes/sim.rs`, `tasks/planning.rs`); **(b)** `grep -r "pv_plan_kw" VEN/src/entities/ VEN/src/controller/` returns empty (domain ring stays clean — `controller/milp_planner/` uses the parameter name `pv_forecast_override`, not `pv_plan_kw`); **(c)** `wc -l VEN/src/tasks/planning.rs` → assert < 200 (Constitution Prin VI tasks-file line limit)
- [x] T024 Update `docs/history/project_journal.md` AND `docs/reference/KEY_LEARNINGS.md` with implementation notes (Constitution Development Workflow §1 and §2): what was changed, why, key learnings — e.g., inject snapshot read-before-`spawn_blocking` in `planning.rs`, clamping negative values, `pv_forecast_override` rename at the domain boundary, and the `AssetStateChange`-vs-plan-timestamp distinction for US3 step implementation
- [ ] T025 Commit all changes on branch `022-deterministic-test-env` with message: `feat(ven): add pv_plan_kw planning forecast override for deterministic BDD tests`

---

## Dependencies & Execution Order

### Phase Dependencies

- **Foundational (Phase 1)**: T000 has no dependencies — write and commit the @wip BDD stub first; T001+T002 depend on T000; T003 depends on T001+T002
- **US1 (Phase 2)**: Depends on Phase 1 completion (T001+T002 must compile via T003); T026 depends on T005
- **US2 (Phase 3)**: Depends on Phase 2 BDD step T006 (needs the step to exist to update feature files); T007–T010 not required before audit starts
- **US3 (Phase 4)**: Depends on T006 (step needed) and T002 (no-replan code in place); T019 also satisfies the @wip stub from T000
- **Polish (Phase 5)**: Depends on all phases complete

### Within Phase 2 (US1)

- T004 and T005 are in different files — can run in parallel; both must compile before T007
- T006 (BDD step) can run in parallel with T004/T005 — different language, different file
- T026 (unit test) depends on T005 (needs `build_milp_inputs` signature with `pv_forecast_override`); can be written in parallel with T004/T006 and finalized after T005
- T007 depends on T004 (run_planner signature must exist)
- T008 depends on T006 (step must exist before the feature file uses it)
- T009 depends on T004+T005+T007+T026 (all Rust changes + unit test written)
- T010 depends on T006+T008+T009

### Within Phase 3 (US2)

- T011–T017 are in different feature files — all can run in parallel after T006
- T018 depends on all of T011–T017

### Within Phase 4 (US3)

- T019 depends on T006 (new step definition needed) and T003 (inject endpoint must compile)
- T020 depends on T002 (no-replan code must be written)
- T021 depends on T019+T020

### Parallel Opportunities

```bash
# Pre-Phase-1 (do first, no deps):
Task T000: tests/features/ven_planner.feature  (@wip stub — BDD-First gate)

# Phase 1 — parallel pair after T000 (compile together at T003):
Task T001: VEN/src/state.rs
Task T002: VEN/src/routes/sim.rs

# Phase 2 — three-way parallel immediately after T003:
Task T004: VEN/src/controller/milp_planner/mod.rs
Task T005: VEN/src/controller/milp_planner/inputs.rs
Task T006: tests/features/steps/phase_a_physics_steps.py
# After T005 (draft in parallel, finalize after T005 compiles):
Task T026: VEN/src/controller/milp_planner/ (unit test for build_milp_inputs)

# Phase 3 — seven-way parallel after T006:
Task T011: tests/features/ven_dispatcher.feature
Task T012: tests/features/use_cases.feature
Task T013: tests/features/ven_planner.feature
Task T014: tests/features/ven_uc_normal.feature
Task T015: tests/features/ven_uc_stress.feature
Task T016: tests/features/asset_forecast.feature        ← NEW
Task T017: tests/features/ven_timeline.feature          ← NEW
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete **Phase 1** — Foundational (T001–T003)
2. Complete **Phase 2** — US1 (T004–T010)
3. **STOP and VALIDATE**: Run `deviation_absorber.feature` at three times of day; confirm passes
4. Proceed to Phase 3–5 for full suite-wide adoption and non-regression

### Incremental Delivery

1. Phase 1 → structs compile → foundation ready
2. Phase 2 → planner override + BDD fix → MVP delivered (scenario:149 unblocked)
3. Phase 3 → suite-wide audit → full non-determinism eliminated
4. Phase 4 → no-replan verified → US3 confirmed
5. Phase 5 → polish + commit → branch ready for review

---

## Notes

- `pv_plan_kw` must never appear in `VEN/src/entities/` — the field is infrastructure-ring only. The domain-facing parameter is named `pv_forecast_override: Option<f64>` and lives only in `milp_planner/mod.rs` and `milp_planner/inputs.rs`; `grep "pv_plan_kw" VEN/src/` finds exactly three files: `state.rs`, `routes/sim.rs`, `tasks/planning.rs`
- The `pv_forecast_override` parameter on `run_planner` is not exposed to callers outside `planning.rs`; it carries the inject snapshot value only
- For T012 (`use_cases.feature`): set `pv_plan_kw` to match the injected `pv_irradiance` value (e.g., `pv_plan_kw=1.0` when `pv_irradiance=1.0`) so the forecast matches what the physics tick sees
- Apply `pv_plan_kw` at Background level when all scenarios in the file need it; apply per-scenario only when some scenarios intentionally test time-varying dispatch
- Always run `--build` when BDD test Python or feature files have changed (Docker caches the image)
- The exact log string indicating a plan cycle fire is `"planner loop: starting plan cycle"` — verified at `VEN/src/tasks/planning.rs:52`
- US3 acceptance scenario 2 ("tariff event fires → subsequent solve uses override") is covered implicitly by the suite-wide Phase 3 adoption: when `pv_plan_kw` is set in a Background and a tariff event fires during any Phase 3 BDD scenario, the override applies
- Commit after T010 (US1 complete) and again after T018 (US2 complete) before T025 final commit
