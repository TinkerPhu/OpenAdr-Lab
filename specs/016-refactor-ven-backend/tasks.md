# Tasks: 016 — Refactor VEN Backend

**Input**: Design documents from `specs/016-refactor-ven-backend/`
**Prerequisites**: plan.md ✅ spec.md ✅ research.md ✅ data-model.md ✅ contracts/ ✅

> ⚠️ **Important clarification for US7**: data-model.md §4 previously described `#[serde(flatten)]` inside a single InnerState — that was incorrect. The spec FR-013 and T049 have both been updated to reflect the correct approach: three *separate* `Arc<RwLock<T>>` locks in `AppState`, with a private `PersistedVenState` helper for JSON serialisation. data-model.md §4 now matches this. T049 is retained in Phase 9 as a verification/clean-up pass.

**Tests**: No new test files. Unit tests added inline in `VEN/src/state.rs` (US2, US7) and `VEN/src/loops.rs` (US6) per SC-003, SC-006. No TDD — tests follow implementation and verify correctness.

**Organization**: Tasks are grouped by user story (8 phases + polish).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel with other [P] tasks in the same phase (different files, no shared dependencies)
- **[USN]**: User story this task belongs to

---

## Phase 1: Setup (Baseline Verification)

**Purpose**: Confirm clean starting state before any changes

- [X] T001 Check out feature branch `016-refactor-ven-backend` and run `cargo check 2>&1 | tee /tmp/ven_check_baseline.txt` in `VEN/`; count warnings with `grep -c "^warning" /tmp/ven_check_baseline.txt` and note the count — this is the SC-001 baseline; proceed only if zero errors

---

## Phase 2: User Story 1 — Remove Phantom Dead File (Priority: P1) 🎯 MVP

**Goal**: Delete `VEN/src/controller/profile.rs` — a 22 KB file that is never compiled because `controller/mod.rs` contains no `mod profile;` declaration.

**Independent Test**: `git rm` + `cargo check` — zero errors; `grep -rn "controller::profile" VEN/src/` — zero hits.

- [X] T002 [US1] Delete `VEN/src/controller/profile.rs` via `git rm VEN/src/controller/profile.rs`
- [X] T003 [US1] Run `cargo check` in `VEN/` and confirm zero errors; run `grep -rn "controller::profile" VEN/src/ --include='*.rs'` and confirm zero matches

**Checkpoint**: controller/profile.rs absent from repository; project compiles cleanly.

---

## Phase 3: User Story 2 — Remove `cancel_request` Legacy Fallback (Priority: P1)

**Goal**: Remove the dead `None =>` arm in `AppState::cancel_request` in `VEN/src/state.rs`; add a `warn!()` for any future None case; confirm all cancel unit tests pass.

**Independent Test**: `cargo test --manifest-path VEN/Cargo.toml -- cancel_request` — all cancel tests pass.

- [X] T004 [US2] In `VEN/src/state.rs`, locate the `match session_type` block in `AppState::cancel_request`; note that `session_type` is `Option<SessionType>` (confirmed in `VEN/src/entities/user_request.rs`); remove only the existing `None => { state.active_requests.retain(...) }` arm that silently no-ops — T005 replaces it with a warn!() arm
- [X] T005 [US2] In `VEN/src/state.rs`, add an explicit `None =>` branch to the `cancel_request` match that calls `tracing::warn!("cancel_request: unexpected session_type: None for request {}", request_id)` and returns `true` (the field IS `Option<SessionType>` — confirmed; this branch guards against any future code path that creates a request without a session type)
- [X] T006 [US2] In `VEN/src/state.rs` `#[cfg(test)]`, add or update unit tests confirming: (a) `SessionType::Ev` cancellation clears `ev_session`; (b) `SessionType::Heater` cancellation clears `heater_target`; (c) `SessionType::ShiftableLoad` cancellation removes the matching load/runtime entry from `shiftable_loads`/`shiftable_runtimes`
- [ ] T007 [US2] Run `cargo test --manifest-path VEN/Cargo.toml` and confirm all state-module tests pass

**Checkpoint**: Legacy None branch gone; warn!() in place; all cancel unit tests green.

---

## Phase 4: User Story 3 — Remove `AssetCapabilities` Dead Code (Priority: P2)

**Goal**: Delete `AssetCapabilities`, `EnergyState`, `TimeWindow`, and all five `capabilities()` method implementations from `VEN/src/assets/mod.rs`.

**Independent Test**: `cargo check`; `grep -n "AssetCapabilities\|EnergyState\|TimeWindow\|fn capabilities" VEN/src/ --include='*.rs'` — zero hits.

- [X] T008 [P] [US3] In `VEN/src/assets/mod.rs`, delete the `struct AssetCapabilities` definition and its `impl AssetCapabilities` block (all methods)
- [X] T009 [P] [US3] In `VEN/src/assets/mod.rs`, delete the `struct EnergyState` and `struct TimeWindow` definitions (used only by AssetCapabilities)
- [X] T010 [US3] In `VEN/src/assets/mod.rs`, delete the `fn capabilities(&self) -> AssetCapabilities` method from each of the five `AssetConfig` variant `impl` blocks (`Battery`, `Ev`, `Pv`, `Heater`, `BaseLoad`)
- [ ] T011 [US3] Run `cargo check` in `VEN/`; confirm zero errors; verify `VEN/src/routes/assets.rs` still compiles and references only `AssetCapability` (singular, the live type) — not `AssetCapabilities`

**Checkpoint**: Dead capability code gone; `GET /capability` route unchanged; project compiles.

---

## Phase 5: User Story 4 — Remove Legacy `DeviceConfig` (Priority: P2)

**Goal**: Delete `DeviceConfig`, remove the `devices` field from `Profile`, simplify all 5 accessor methods, add startup guard for empty `assets`, and update `main.rs` to hard-fail on bad profile.

**Independent Test**: `cargo test --manifest-path VEN/Cargo.toml -- profile` — all profile tests pass including the new startup-guard test.

- [X] T012 [US4] In `VEN/src/profile.rs`, remove the `#[serde(default)] pub devices: DeviceConfig` field from `struct Profile`
- [X] T013 [US4] In `VEN/src/profile.rs`, delete the entire `struct DeviceConfig` definition, its `impl Default for DeviceConfig` block, and the `fn default_base_load() -> f64` free function
- [X] T014 [US4] In `VEN/src/profile.rs`, simplify `ev_config()`, `heater_config()`, `pv_config()`, and `battery_config()` — remove the trailing `.or(self.devices.X.as_ref())` fallback arm from each; each method body becomes a single `self.assets.iter().find_map(|a| if let AssetProfile::X(c) = a { Some(c) } else { None })` call
- [X] T015 [US4] In `VEN/src/profile.rs`, simplify `base_load_kw()` — replace `.unwrap_or(self.devices.base_load_w / 1000.0)` with `.unwrap_or_else(default_base_load_kw)` (0.5 kW default, matching the existing `default_base_load_kw()` fn)
- [X] T016 [US4] In `VEN/src/profile.rs`, add post-parse validation inside `try_load()` immediately after `serde_yaml::from_str(&contents)?`: `if profile.assets.is_empty() { anyhow::bail!("Profile has no assets — check for legacy 'devices:' key (got {} bytes)", contents.len()); }`
- [X] T017 [US4] In `VEN/src/main.rs`, replace the `Profile::load(path).await` call with `Profile::try_load(path).await?` so a failed profile (including empty-assets guard) propagates to `main()` and exits with non-zero status
- [X] T018 [US4] In `VEN/src/profile.rs` `#[cfg(test)]`, add a unit test `profile_empty_assets_guard` that writes a temp YAML file containing only a `devices:` block (no `assets:` key), calls `Profile::try_load()`, and asserts it returns `Err` containing `"Profile has no assets"`
- [ ] T019 [US4] Run `cargo test --manifest-path VEN/Cargo.toml` and confirm all profile module tests pass (including T018)

**Checkpoint**: DeviceConfig removed; startup guard active; all profile tests green.

---

## Phase 6: User Story 5 — Centralize Asset ID Constants (Priority: P2)

**Goal**: Create `VEN/src/ids.rs` with 6 asset-ID constants; replace all inline asset-ID string literals in production source; add boiler gap comment.

**Independent Test**: `grep -rn '"heater"\|"boiler"\|"ev"\|"battery"\|"pv"\|"base_load"' VEN/src/ --include='*.rs' | grep -v test | grep -v "\.yaml"` — zero hits.

- [X] T020 [US5] Create `VEN/src/ids.rs` with six `pub const` definitions: `EV: &str = "ev"`, `BATTERY: &str = "battery"`, `PV: &str = "pv"`, `HEATER: &str = "heater"`, `BOILER: &str = "boiler"`, `BASE_LOAD: &str = "base_load"`
- [X] T021 [US5] In `VEN/src/main.rs`, add `mod ids;` after the existing `mod` block (alphabetical order)
- [X] T022 [P] [US5] Run `grep -rn '"heater"\|"boiler"\|"ev"\|"battery"\|"pv"\|"base_load"' VEN/src/ --include='*.rs' | grep -v test | grep -v "\.yaml"` to list all production literal sites; replace each with the corresponding `crate::ids::*` constant — **FR-007 has no exemptions for production call sites**, including `default_asset_id_*()` functions that return these IDs as serde defaults (replace `"heater".into()` with `crate::ids::HEATER.into()` etc.); exempt only: serde `rename` attributes and test assertion literals
- [X] T023 [US5] In `VEN/src/routes/hems.rs` at the dual-match site (search for `"heater" || "boiler"` or equivalent), add the comment: `// TODO(boiler-physics): Boiler (200L DHW) and Heater (2000L space-heating) share the HEMS session path. Full boiler dispatcher/planner propagation requires its own physics model — deferred to a future feature.`
- [X] T024 [US5] Run `cargo check` in `VEN/`; rerun the grep from T022 and confirm zero non-test production literal hits; confirm `ids.rs` constants are used at every former literal site

**Checkpoint**: ids module exists with 6 constants; zero bare asset-ID literals in production code; boiler gap documented.

---

## Phase 7: User Story 6 — Extract `spawn_sim_tick` Phases (Priority: P3)

**Goal**: Decompose the monolithic `spawn_sim_tick` body in `VEN/src/loops.rs` into ≥6 named phase functions; `spawn_sim_tick` becomes a pure orchestrator; add ≥1 unit test calling a phase function without `AppCtx`.

**Independent Test**: `cargo test --manifest-path VEN/Cargo.toml -- sim_tick`; at least one new phase test passes; `wc -l` on each extracted function ≤60 lines (SC-005).

- [X] T025 [US6] Read the `spawn_sim_tick` body in `VEN/src/loops.rs` and mark the logical phase boundaries (expected ≥6: injection application, setpoint build, physics tick + history, sim-state publish/SSE, report obligation, planner trigger + persist counter) **by adding inline `// PHASE N: <name>` comments directly in the source** — these committed comments serve as the reviewable handoff artefact before T026–T030 begin; do not extract any code in this task
- [X] T026 [US6] In `VEN/src/loops.rs`, extract the injection-application phase into `fn apply_sim_injections(inject: SimInjectState, sim: &mut SimState) -> Vec<ClearedInjectField>` (or equivalent) — function is synchronous, takes owned/mutable inputs, returns cleared-field list; caller applies the AppState clears after the function returns
- [X] T027 [P] [US6] In `VEN/src/loops.rs`, extract the setpoint-build phase into `fn build_setpoints(plan: Option<&Plan>, capacity: &OadrCapacityState, tariffs: &[TariffSnapshot], profile: &Profile, ev_settings: &EvSettings, now: DateTime<Utc>) -> Setpoints` (exact types from existing code) — no mutex, no AppState reference
- [X] T028 [P] [US6] In `VEN/src/loops.rs`, extract the deviation/correction phase (Plan F/G: deviation_ticks, correction_is_active, prev_correction_kw state machine) into `fn apply_deviation_correction(state: &mut DeviationState, setpoints: &mut Setpoints, profile: &PlannerConfig)` where `DeviationState` is a small stack-local struct holding the three mutable counters
- [X] T029 [P] [US6] In `VEN/src/loops.rs`, extract the sim-state publish phase (post-tick SSE broadcast, AppState sensor/sim update, trigger_tx send logic) into a named async function that takes the tick result snapshot and the relevant channels/handles
- [X] T030 [US6] In `VEN/src/loops.rs`, extract the persist-counter phase and report-obligation trigger into named functions; rewrite the `spawn_sim_tick` loop body as a clean orchestrator: snapshot → apply_injections → build_setpoints → correction → sim.tick → publish → report_obligation → persist_counter
- [X] T031 [US6] In `VEN/src/loops.rs` `#[cfg(test)]`, add unit test `test_build_setpoints_no_plan` that calls the extracted setpoint-build function with `plan: None` and a synthetic profile — confirms the function returns a default/zero setpoints struct without needing `AppCtx`, a running sim loop, or a mutex
- [ ] T032 [US6] Run `cargo test --manifest-path VEN/Cargo.toml`; confirm all existing tests + T031 pass; confirm no extracted function body in `VEN/src/loops.rs` exceeds 60 lines

**Checkpoint**: spawn_sim_tick is an orchestrator of named functions; ≥1 phase unit test passes; all tests green.

---

## Phase 8: User Story 7 — Split `InnerState` into Three Independent Locks (Priority: P3)

**Goal**: Replace `AppState`'s single `Arc<RwLock<InnerState>>` with three independent `Arc<RwLock<T>>` fields so polling reads and sim-tick writes no longer contend; preserve `state.json` format; add INVARIANT comment.

**Independent Test**: `cargo test --manifest-path VEN/Cargo.toml -- state`; persistence round-trip test passes; programs read and ctrl_sim write operate on separate locks (verified by inspection).

### Define sub-structs

- [X] T033 [US7] In `VEN/src/state.rs`, define `pub struct PollingState { pub programs: Vec<serde_json::Value>, pub events: Vec<serde_json::Value>, pub reports: Vec<serde_json::Value> }` with `#[derive(Debug, Clone, Serialize, Deserialize, Default)]`
- [X] T034 [US7] In `VEN/src/state.rs`, define `pub struct ControllerSimState` with fields: `pub sensor: SensorSnapshot`, `#[serde(skip)] pub sim: Option<SimSnapshot>`, `#[serde(skip)] pub inject_state: SimInjectState`, `#[serde(skip)] pub controller_trace: ControllerTrace`; derive `Debug, Clone, Serialize, Deserialize`; add `impl Default` initialising `sensor` with `SensorSnapshot::empty_now()` and remaining fields with their own defaults
- [X] T035 [US7] In `VEN/src/state.rs`, define `pub struct HemsState` containing the 13 HEMS fields (`active_plan`, `planned_tariffs`, `capacity_state`, `report_obligations`, `asset_ledger`, `active_requests`, `site_envelope`, `ev_session`, `heater_target`, `shiftable_loads`, `shiftable_runtimes`, `baseline_override`, `ev_settings`) with `#[derive(Debug, Clone, Default)]` — no serde (all are runtime-only)

### Refactor AppState

- [X] T036 [US7] In `VEN/src/state.rs`, replace `struct AppState { inner: Arc<RwLock<InnerState>> }` with `pub struct AppState { pub polling: Arc<RwLock<PollingState>>, pub ctrl_sim: Arc<RwLock<ControllerSimState>>, pub hems: Arc<RwLock<HemsState>> }`; delete the `InnerState` struct and its manual `Clone` impl; add the following comment at the top of `impl AppState`: `// INVARIANT: No function may acquire more than one lock simultaneously. Always snapshot-and-release: acquire → clone needed fields → drop guard → work on snapshot. No guard may cross an .await point or a second lock acquisition.`
- [X] T037 [US7] In `VEN/src/state.rs`, rewrite `AppState::new()` to initialise: `polling: Arc::new(RwLock::new(PollingState::default()))`, `ctrl_sim: Arc::new(RwLock::new(ControllerSimState::default()))`, `hems: Arc::new(RwLock::new(HemsState::default()))`
- [X] T038 [US7] In `VEN/src/state.rs`, update all `PollingState`-related accessor methods (`set_programs`, `programs`, `set_events`, `events`, `set_reports`, `reports`, and any `add_report` / `clear_reports` helpers) to acquire `self.polling.write().await` or `self.polling.read().await`
- [X] T039 [US7] In `VEN/src/state.rs`, update all `ControllerSimState`-related accessor methods (`sensor`, `set_sensor`, `sim_snapshot`/`set_sim_snapshot`, `inject_state`, `set_inject_state`, `clear_inject_field`, `controller_trace`, `set_controller_trace`) to acquire `self.ctrl_sim.write().await` or `self.ctrl_sim.read().await`
- [X] T040 [US7] In `VEN/src/state.rs`, update all `HemsState`-related accessor methods (active_plan, set_active_plan, planned_tariffs, set_planned_tariffs, capacity_state, set_capacity_state, report_obligations, add_report_obligation, asset_ledger, update_ledger, active_requests, add_request, cancel_request, site_envelope, ev_session, set_ev_session, heater_target, set_heater_target, shiftable_loads, shiftable_runtimes, baseline_override, ev_settings, set_ev_settings) to acquire `self.hems.write().await` or `self.hems.read().await`
- [X] T041 [US7] In `VEN/src/state.rs`, define a private `#[derive(Serialize, Deserialize)] struct PersistedVenState { programs: Vec<serde_json::Value>, events: Vec<serde_json::Value>, reports: Vec<serde_json::Value>, sensor: SensorSnapshot }` and rewrite `load_from_json` (acquire polling.write + ctrl_sim.write separately, distribute deserialized fields) and `to_json` (acquire polling.read + ctrl_sim.read separately, assemble into PersistedVenState, serialize) — `hems` is all runtime-only and not persisted
- [X] T042 [US7] In `VEN/src/state.rs` `#[cfg(test)]`, add unit test `test_state_persistence_roundtrip`: set `programs = vec![json!({"id":"p1"})]` via polling lock; set `sensor.power_kw = 3.5` via ctrl_sim lock; call `to_json()`; construct a new `AppState`; call `load_from_json()`; assert programs and sensor survive the round-trip unchanged

### Fix compile errors and validate

- [ ] T043 [US7] Run `cargo check` in `VEN/` and fix all compile errors arising from accessor path changes across `VEN/src/loops.rs`, `VEN/src/routes/`, `VEN/src/controller/mod.rs`, and `VEN/src/simulator/` (expected: callers of AppState are unaffected since method signatures are unchanged — only internal field paths in state.rs changed); additionally, audit each updated accessor from T038–T040 for FR-014 compliance: no lock guard held across an `.await` point or a second lock acquisition; fix any violations found
- [ ] T044 [US7] Run `cargo test --manifest-path VEN/Cargo.toml` and confirm all tests pass including T042

**Checkpoint**: AppState has 3 independent locks; persistence round-trip verified; all tests green; polling reads no longer share a lock with sim-tick writes.

---

## Phase 9: Polish & Cross-Cutting Concerns

- [ ] T045 [P] Run `cargo test --workspace` from repo root — zero failures; zero new warnings vs T001 baseline (SC-001, SC-003); verify SC-007 by inspection — confirm `AppState::programs()` acquires `self.polling` and that no sim-tick write path acquires `self.polling` (locks are independent); note SC-007 verification result in the commit message
- [X] T046 [P] Run `grep -rn "DeviceConfig\|AssetCapabilities\|EnergyState\|TimeWindow\|fn capabilities" VEN/src/ --include='*.rs'` — confirm zero hits (SC-002) [NOTE: `TimeWindow` hits found in `controller/timeline.rs` are a different live struct; the dead `TimeWindow` from `assets/mod.rs` is absent — SC-002 satisfied]
- [ ] T047 Run full BDD regression on Pi4-Server: `ssh Pi4-Server && cd /srv/docker/openadr_lab && git fetch && git checkout 016-refactor-ven-backend && git pull && docker compose -f tests/docker-compose.test.yml run --build --rm test-runner` — all scenarios pass (SC-004)
- [X] T048 [P] Verify SC-005: for each extracted phase function in `VEN/src/loops.rs`, confirm line count ≤60 lines; log any over-limit functions for further extraction [NOTE: `apply_deviation_correction` ~94 lines, `publish_sim_tick_result` ~127 lines — both exceed SC-005; logged as candidates for future decomposition; `apply_sim_injections` ~30 lines, `build_tick_setpoints` ~50 lines pass]
- [X] T049 [P] Update `specs/016-refactor-ven-backend/data-model.md` §4 — correct the `serde(flatten)` description: `AppState` holds 3 separate `Arc<RwLock<T>>` (per FR-013), with persistence handled via `PersistedVenState` helper; remove the incorrect nested-InnerState diagram [VERIFIED: data-model.md §4 already has the correct 3-lock content]
- [X] T050 [P] Append a summary entry to `docs/history/project_journal.md`: list items completed (R-01 through R-07), key decisions made (PersistedVenState helper for backwards-compatible JSON, startup guard in `try_load()`, `ControllerSimState` naming to avoid `simulator::SimState` collision), and the date — required by Constitution §Dev-Workflow §1
- [X] T051 [P] Add an entry to `docs/reference/KEY_LEARNINGS.md`: document (a) the InnerState → 3-lock split pattern and why `PersistedVenState` is needed for JSON backwards-compatibility, (b) the startup guard placement in `try_load()` vs `load()` and why `load()` must remain for tests, (c) the `ControllerSimState` naming decision — required by Constitution §Dev-Workflow §2

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Setup)**: No dependencies — start immediately
- **Phase 2 (US1)**: No dependencies — can start after Phase 1
- **Phase 3 (US2)**: Independent from US1 (different file: `state.rs`) — can run in parallel with Phase 2
- **Phase 4 (US3)**: Independent (file: `assets/mod.rs`) — can start after Phase 1
- **Phase 5 (US4)**: Independent (files: `profile.rs`, `main.rs`) — can start after Phase 1
- **Phase 6 (US5)**: Recommended after Phases 2–5 (literal sweep is simpler on a cleaner codebase); `ids.rs` has no technical blockers
- **Phase 7 (US6)**: Independent (file: `loops.rs`) — can start after Phase 1; recommended after Phase 6 so extracted functions can use `ids::*` constants directly
- **Phase 8 (US7)**: Recommended last among user stories — touches `state.rs` (overlap with Phase 3); all other stories should be merged first to avoid conflicts
- **Phase 9 (Polish)**: Depends on Phases 2–8 all complete

### Story Dependency Graph

```
Phase 1 (baseline)
    │
    ├──► Phase 2 (US1: controller/profile.rs) ─────────────┐
    ├──► Phase 3 (US2: state.rs cancel_request) ───────────┤
    ├──► Phase 4 (US3: assets/mod.rs) ─────────────────────┤──► Phase 8 (US7) ──► Phase 9
    ├──► Phase 5 (US4: profile.rs) ────────────────────────┤
    ├──► Phase 6 (US5: ids.rs + literal sweep) ────────────┤
    └──► Phase 7 (US6: loops.rs extract) ──────────────────┘
```

Recommended sequential order for one developer: **1 → 2 → 3 → 4 → 5 → 6 → 7 → 8 → 9**

### Parallel Opportunities (within a phase)

- **T008 + T009** (Phase 4): Both in `assets/mod.rs`, non-overlapping sections — parallelizable
- **T027 + T028 + T029** (Phase 7): Three independent phase extractions in `loops.rs`
- **T033 + T034 + T035** (Phase 8): Three independent struct definitions in `state.rs`
- **T045 + T046 + T048 + T049** (Phase 9): All verification tasks, no side effects

---

## Parallel Example: Phase 4 + Phase 5 + Phase 6 (all P2, different files)

```
# Start simultaneously (independent files):
Thread A → T008/T009/T010: Delete AssetCapabilities from VEN/src/assets/mod.rs
Thread B → T012/T013:      Remove DeviceConfig from VEN/src/profile.rs
Thread C → T020/T021:      Create VEN/src/ids.rs

# After Thread A done:
Thread A → T011: cargo check for assets

# After Thread B done:
Thread B → T014/T015/T016/T017/T018: Simplify accessors + guard + main.rs

# After all three threads complete:
Thread A → T022: literal sweep (benefits from cleaner profile.rs from Thread B)
```

---

## Implementation Strategy

### MVP First (User Stories 1 + 2 Only — P1)

1. Complete Phase 1: baseline
2. Complete Phase 2: US1 (delete dead file)
3. Complete Phase 3: US2 (remove None branch)
4. **STOP and VALIDATE**: `cargo test` passes; 2 simplest debt items cleared
5. Open draft PR for early review

### Incremental Delivery

1. Phase 1 → Baseline confirmed
2. Phases 2 + 3 → P1 debts cleared (trivial, zero-risk)
3. Phases 4 + 5 + 6 → P2 debts cleared (DeviceConfig, dead capability code, ids module)
4. Phase 7 → spawn_sim_tick decomposed; phase unit test added
5. Phase 8 → Lock architecture modernised; polling/sim contention eliminated
6. Phase 9 → Full regression; BDD on Pi4; polish

### Two-Developer Strategy

- **Developer A**: Phases 2 + 3 + 7 (US1, US2, US6 — file-isolated: controller/, state.rs, loops.rs)
- **Developer B**: Phases 4 + 5 + 6 (US3, US4, US5 — file-isolated: assets/, profile.rs, ids.rs)
- After both complete: jointly tackle Phase 8 (US7 — most disruptive) then Phase 9

---

## Notes

- No BDD test file modifications expected (FR-015: behaviour-preserving)
- Test assertion string literals (`"heater"`, `"ev"`, etc. in `assert_eq!`) are **exempt** from the T022 literal sweep
- `default_asset_id_*()` functions in `profile.rs` that return `"heater".into()` etc. can keep their literals — they produce default YAML id values, not asset-type discriminants
- US7 (Phase 8) is the highest-risk change — run `cargo check` after each batch of accessor updates (T038, T039, T040) rather than waiting for all three to be done
- The `PersistedVenState` struct in T041 must use the exact JSON key names (`programs`, `events`, `reports`, `sensor`) to preserve backwards-compatibility with existing `state.json` files on Pi4
- Commit after each Phase checkpoint for clean `git bisect` history
- [P] tasks = safe to parallelize (different files, no shared state)
