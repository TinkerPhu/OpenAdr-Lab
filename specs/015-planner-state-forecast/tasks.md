# Tasks: Planner State Forecast in Timeline API

**Input**: Design documents from `/specs/015-planner-state-forecast/`  
**Branch**: `015-planner-state-forecast`  
**Prerequisites**: plan.md ✅, spec.md ✅, contracts/ ✅

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1 = Battery SoC, US2 = EV SoC, US3 = Heater T_tank)
- Exact file paths are given in each description

---

## Phase 1: Setup (Baseline Verification)

**Purpose**: Confirm starting state before any code changes.

- [ ] T001 Run `cargo test --workspace` in `VEN/` and confirm all tests pass (establishes regression baseline)

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Data-model and wiring changes that must compile cleanly before any user story can add behavior.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete. After this phase, `cargo test` must still pass with no behavior change.

- [ ] T002 [P] Add `planned_state_by_asset: HashMap<String, HashMap<String, f64>>` field (with `#[serde(default)]`) to `PlanTimeSlot` struct in `VEN/src/entities/plan.rs`
- [ ] T003 [P] Add `soc_ev_init: Option<f64>` field to `MilpInputs` struct in `VEN/src/controller/milp_planner.rs` and update the `make_solver_inputs` test helper struct literal with `soc_ev_init: None` so it compiles
- [ ] T004 Populate `soc_ev_init` from the live EV asset state in `build_milp_inputs` in `VEN/src/controller/milp_planner.rs`: extract `AssetState::Ev(s).soc` when an EV asset entry exists in `sim`, store as `Some(soc)`; `None` when absent (depends on T003)
- [ ] T005 Merge `planned_state_by_asset` into future point values in `build_asset_timeline` in `VEN/src/controller/timeline.rs`: after the existing `values.insert("power_kw", ...)` block, add `if let Some(state_map) = slot.planned_state_by_asset.get(asset_id) { values.extend(...) }` (depends on T002)

**Checkpoint**: `cargo test --workspace` passes — all tests green, no behavior change yet

---

## Phase 3: User Story 1 — Battery SoC Forecast in Timeline (Priority: P1) 🎯 MVP

**Goal**: Future battery timeline points include a `soc` key (0.0–1.0) derived from the MILP energy trajectory.

**Independent Test**: `GET /timeline/battery?hours_forward=4` returns future points that each contain `"soc"` in their `values` map, with values consistent with the MILP plan. Verify with `cargo test` unit tests; BDD smoke check in Phase 6.

- [ ] T006 [P] [US1] Add `Battery::future_state_values(&self, e_kwh: f64) -> HashMap<String, f64>` method near `state_values()` in `VEN/src/assets/battery.rs`: compute `soc = (e_kwh / self.capacity_kwh).clamp(0.0, 1.0)`, return `[("soc".to_string(), soc)].into()`
- [ ] T007 [US1] Add unit tests for `Battery::future_state_values` in `VEN/src/assets/battery.rs`: `battery_future_state_mid_soc` (e_kwh = 5.0, capacity = 10.0 → soc = 0.5), `battery_future_state_clamp_over` (e_kwh > capacity → soc = 1.0), `battery_future_state_clamp_under` (e_kwh < 0 → soc = 0.0) (depends on T006)
- [ ] T008 [US1] Populate `planned_state_by_asset` for battery in `translate_to_plan` in `VEN/src/controller/milp_planner.rs`: build `Battery::from_config(battery_cfg)`, iterate slots, for each slot `t` call `battery.future_state_values(sol.e_bat_kwh[t])` and insert into `slot.planned_state_by_asset` under key `battery_id` (depends on T002, T006)
- [ ] T009 [US1] Add unit test `translate_to_plan_battery_slot_has_soc` in `VEN/src/controller/milp_planner.rs`: run a minimal solve with battery enabled, call `translate_to_plan`, assert every slot's `planned_state_by_asset["battery"]["soc"]` is in [0.0, 1.0] (depends on T008)
- [ ] T010 [US1] Add unit test `future_battery_point_includes_soc` in `VEN/src/controller/timeline.rs`: construct a `Plan` with battery slots that have `planned_state_by_asset["battery"]["soc"] = 0.75`, call `build_asset_timeline`, assert the future battery point has `values["soc"] == 0.75` (depends on T005, T008)

**Checkpoint**: `cargo test --workspace` passes — battery future points carry `soc` values

---

## Phase 4: User Story 2 — EV SoC Forecast in Timeline (Priority: P1)

**Goal**: Future EV timeline points include a `soc` key derived by integrating the planned EV charge power forward from the live SoC at plan time.

**Independent Test**: `GET /timeline/ev?hours_forward=4` with an active EV session returns future points with `"soc"` in `values`, starting from the current EV SoC and rising through charging slots. Verify with `cargo test` unit tests.

- [ ] T011 [P] [US2] Add `EvCharger::soc_trajectory(p_ev_kw: &[f64], soc_init: f64, battery_kwh: f64, dt_h: f64) -> Vec<f64>` (returns `Vec` of length `n+1`, index 0 = `soc_init`, `soc[t+1] = (soc[t] + p_ev_kw[t] * dt_h / battery_kwh).clamp(0.0, 1.0)`) and `EvCharger::future_state_values_at(soc: f64) -> HashMap<String, f64>` (returns `{"soc": soc.clamp(0.0, 1.0)}`) near `state_values()` in `VEN/src/assets/ev.rs`
- [ ] T012 [US2] Add unit tests in `VEN/src/assets/ev.rs`: `soc_trajectory_charging` (verify SoC rises correctly over 4 charging slots), `soc_trajectory_clamp_at_full` (charging past 100% clamped to 1.0), `soc_trajectory_empty_input` (empty `p_ev_kw` → `Vec` of length 1 = `[soc_init]`), `future_state_values_at_mid` (soc = 0.6 → map has `"soc" = 0.6`) (depends on T011)
- [ ] T013 [US2] Populate `planned_state_by_asset` for EV in `translate_to_plan` in `VEN/src/controller/milp_planner.rs`: when `inputs.soc_ev_init` is `Some(soc_init)`, call `EvCharger::soc_trajectory(&sol.p_ev_kw, soc_init, ev_cfg.battery_kwh, dt_h)` and for each slot `t` call `EvCharger::future_state_values_at(traj[t])` and insert into `slot.planned_state_by_asset` under the EV asset ID; skip entirely when `soc_ev_init` is `None` (depends on T002, T004, T011)
- [ ] T014 [US2] Add unit test `translate_to_plan_ev_slot_has_soc` in `VEN/src/controller/milp_planner.rs`: run a minimal solve with EV MustRun mode, set `soc_ev_init = Some(0.5)` on the `MilpInputs`, call `translate_to_plan`, assert charging slots have `planned_state_by_asset["ev"]["soc"] > 0.5` and SoC is monotonically non-decreasing across charging slots (depends on T013)

**Checkpoint**: `cargo test --workspace` passes — EV future points carry `soc` trajectory

---

## Phase 5: User Story 3 — Heater T_tank Forecast in Timeline (Priority: P2)

**Goal**: Future heater timeline points include a `temp_c` key derived from the MILP tank energy trajectory using plan-time thermal model parameters.

**Independent Test**: `GET /timeline/heater?hours_forward=4` returns future points with `"temp_c"` in `values`. Values are within the configured `[temp_min_c, temp_max_c]` band (or a small tolerance). Verify with `cargo test` unit tests.

- [ ] T015 [P] [US3] Add `Heater::future_state_values(&self, e_tank_kwh: f64) -> HashMap<String, f64>` method near `state_values()` in `VEN/src/assets/heater.rs`: compute `temp_c = self.temp_min_c + e_tank_kwh / self.thermal_mass_kwh_per_c`, return `[("temp_c".to_string(), temp_c)].into()`
- [ ] T016 [US3] Add unit tests in `VEN/src/assets/heater.rs`: `heater_future_state_at_min` (e_tank_kwh = 0.0 → temp_c == temp_min_c), `heater_future_state_above_min` (e_tank_kwh > 0 → temp_c > temp_min_c, verify arithmetic) (depends on T015)
- [ ] T017 [US3] Populate `planned_state_by_asset` for heater in `translate_to_plan` in `VEN/src/controller/milp_planner.rs`: build `Heater::from_config(heater_cfg)`, guard on `!sol.e_heat_tank_kwh.is_empty()`, for each slot `t` call `heater.future_state_values(sol.e_heat_tank_kwh[t])` and insert into `slot.planned_state_by_asset` under the heater asset ID (depends on T002, T015)
- [ ] T018 [US3] Add unit test `translate_to_plan_heater_slot_has_temp_c` in `VEN/src/controller/milp_planner.rs`: run a minimal solve with heater MustRun mode, call `translate_to_plan`, assert every slot's `planned_state_by_asset["heater"]["temp_c"]` is `>= heater_cfg.temp_min_c` (depends on T017)

**Checkpoint**: `cargo test --workspace` passes — all three assets emit future state values

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: BDD coverage, final verification, documentation.

- [ ] T019 [P] Add BDD scenario `@planner-state` for future battery `soc` key to `tests/features/ven_timeline.feature`: "Given the VEN is running / When I GET /timeline/battery?hours_forward=4 / Then the future battery points include a soc key"
- [ ] T020 [P] Add BDD scenario `@planner-state` for future heater `temp_c` key to `tests/features/ven_timeline.feature`: "Given the VEN is running / When I GET /timeline/heater?hours_forward=4 / Then the future heater points include a temp_c key"
- [ ] T021 Add BDD step definitions for `@planner-state` scenarios in `tests/features/steps/ven_timeline_steps.py`: steps `"future {asset} points include a {key} key"` — GET the timeline, filter to future points (ts > now), assert at least one has `values[key]` present
- [ ] T022 Run `cargo test --workspace` in `VEN/` and confirm no regressions (full green before merge)
- [ ] T023 [P] Append feature entry `015 Planner State Forecast in Timeline API` to `docs/history/project_journal.md`

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Phase 1 baseline. Blocks all user story phases
- **US1 Battery (Phase 3)**: Depends on Phase 2 (T002, T005). No dependency on US2 or US3
- **US2 EV (Phase 4)**: Depends on Phase 2 (T002, T003, T004). No dependency on US1 or US3
- **US3 Heater (Phase 5)**: Depends on Phase 2 (T002). No dependency on US1 or US2
- **Polish (Phase 6)**: Depends on all user story phases desired for release

### User Story Dependencies

- **US1 (P1)**: Independent after Foundational phase
- **US2 (P1)**: Independent after Foundational phase
- **US3 (P2)**: Independent after Foundational phase
- US1 and US2 are both P1 — the `translate_to_plan` edits (T008 and T013) are in the same function; do US1 first, then US2 adds to the same block

### Within Each User Story

- Asset method (T006 / T011 / T015) → unit tests on asset method → populate in `translate_to_plan` → unit test in `translate_to_plan` → unit test in `timeline.rs` (US1 only; timeline merge is shared)

### Parallel Opportunities

Within Phase 2:
- T002 (plan.rs) || T003 (milp_planner.rs struct) — different files, no dependency

Within Phase 3–5 (if working across stories simultaneously):
- T006 (battery.rs) || T011 (ev.rs) || T015 (heater.rs) — different files

---

## Parallel Example: Phase 2

```bash
# These two tasks touch different files and can be done in parallel:
Task T002: Add planned_state_by_asset to PlanTimeSlot in VEN/src/entities/plan.rs
Task T003: Add soc_ev_init to MilpInputs in VEN/src/controller/milp_planner.rs

# Then sequentially:
Task T004: Populate soc_ev_init in build_milp_inputs (same file as T003)
Task T005: Merge planned_state_by_asset in timeline.rs (depends on T002)
```

## Parallel Example: Asset Method Phase

```bash
# After Phase 2, asset method tasks are in different files and can proceed together:
Task T006 [US1]: Battery::future_state_values in VEN/src/assets/battery.rs
Task T011 [US2]: EvCharger::soc_trajectory in VEN/src/assets/ev.rs
Task T015 [US3]: Heater::future_state_values in VEN/src/assets/heater.rs
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup (baseline T001)
2. Complete Phase 2: Foundational (T002–T005)
3. Complete Phase 3: US1 Battery SoC (T006–T010)
4. **STOP and VALIDATE**: `cargo test --workspace` green, manually query `/timeline/battery` to confirm `soc` key appears in future points
5. Add BDD smoke test (T019, T021) and merge

### Incremental Delivery

1. Phase 1 + Phase 2 → compile-clean, no behavior change
2. Phase 3 (Battery) → battery future `soc` visible in timeline (MVP!)
3. Phase 4 (EV) → EV future `soc` trajectory visible
4. Phase 5 (Heater) → heater future `temp_c` visible
5. Phase 6 → BDD coverage + journal, ready to merge

---

## Summary

| Phase | Tasks | Story | Key Files |
|-------|-------|-------|-----------|
| 1 Setup | T001 | — | (none changed) |
| 2 Foundational | T002–T005 | — | `entities/plan.rs`, `milp_planner.rs`, `timeline.rs` |
| 3 Battery SoC | T006–T010 | US1 (P1) | `assets/battery.rs`, `milp_planner.rs`, `timeline.rs` |
| 4 EV SoC | T011–T014 | US2 (P1) | `assets/ev.rs`, `milp_planner.rs` |
| 5 Heater T_tank | T015–T018 | US3 (P2) | `assets/heater.rs`, `milp_planner.rs` |
| 6 Polish | T019–T023 | — | `ven_timeline.feature`, `ven_timeline_steps.py`, `project_journal.md` |

**Total tasks**: 23 (T001–T023)  
**MVP scope**: T001–T010 (battery SoC forecast, 10 tasks)  
**Parallel opportunities**: T002‖T003, T006‖T011‖T015  
**Commits suggested**: after each phase checkpoint, after T022 (final green)
