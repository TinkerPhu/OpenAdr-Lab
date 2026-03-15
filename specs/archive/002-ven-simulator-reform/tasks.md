# Tasks: VEN Simulator Reform

**Input**: Design documents from `specs/002-ven-simulator-reform/`
**Prerequisites**: plan.md ✅, spec.md ✅, research.md ✅, data-model.md ✅, contracts/ ✅

**Tests**: No new BDD scenarios required (pure refactor). Existing 123 scenarios are the acceptance gate. Step definition updates (not feature file changes) are included where the refactor changes response shapes or removes API fields.

**Organization**: Tasks follow the 10 implementation steps from plan.md, mapped to user stories. All steps are sequential at the phase boundary due to Rust type dependencies. Parallelism exists within phases (different files, same type signature target).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no shared dependency on in-progress tasks)
- **[Story]**: Which user story this task belongs to (US1–US5)
- Paths are relative to repo root

---

## Phase 1: Setup (Module Structure)

**Purpose**: Create the `simulator/assets/` module skeleton so all per-type files have a home. All downstream tasks depend on this directory existing and `mod.rs` declaring the types.

- [X] T001 Create `VEN/src/simulator/assets/mod.rs` with: `AssetState` enum (variants only, no method bodies yet), `TickEnvironment` type alias, `AssetCapabilities`, `EnergyState`, `TimeWindow`, `ControlDescriptor`, `ControlKind` structs/enums — all types from data-model.md Planning Interface section

**Checkpoint**: `cargo build` must compile (AssetState variants will be empty stubs; methods added in Phase 2)

---

## Phase 2: Foundational — AssetState Interface (Blocking Prerequisite)

**Purpose**: Implement all 8 required methods on every asset type. This is the core of the refactor — every downstream phase depends on these method signatures being stable and all types compiling.

**⚠️ CRITICAL**: No user story work can begin until all T002–T008 compile successfully.

- [X] T002 [P] Implement `EvCharger` and `EvConfig` in `VEN/src/simulator/assets/ev.rs`: move from `actors.rs`; update `update()` signature to `(dt_s: f64, setpoint: f64, env: &TickEnvironment) -> f64`; implement all 8 methods (`update`, `predict`, `state_values`, `default_setpoint`, `capabilities`, `control_schema`, `reset`, `update_config`)
- [X] T003 [P] Implement `Heater` and `HeaterConfig` in `VEN/src/simulator/assets/heater.rs`: move from `actors.rs`; update `update()` to read `ambient_temp_c` from env; implement all 8 methods
- [X] T004 [P] Implement `PvInverter` and `PvConfig` in `VEN/src/simulator/assets/pv.rs`: move from `actors.rs`; update `update()` to read `hour_of_day` from env; `is_flexible: false` in capabilities; implement all 8 methods
- [X] T005 [P] Implement `Battery` and `BatteryConfig` in `VEN/src/simulator/assets/battery.rs`: move from `actors.rs`; implement all 8 methods; `reset()` sets SoC directly; `update_config()` updates `capacity_kwh`
- [X] T006 [P] Implement `BaseLoad` and `BaseLoadConfig` in `VEN/src/simulator/assets/base_load.rs` (new type — no actor.rs source): `update()` always returns `baseline_kw`; `is_flexible: false`; `default_setpoint()` returns `baseline_kw`; implement all 8 methods
- [X] T007 Wire full `AssetState` enum delegation in `VEN/src/simulator/assets/mod.rs`: add `AssetState::Ev`, `Heater`, `Pv`, `Battery`, `BaseLoad` variants with inner types; implement all 8 enum methods as match-based delegation to inner types (depends on T002–T006)
- [X] T008 Update `VEN/src/simulator/mod.rs`: change import from `actors::*` to `assets::*`; delete `VEN/src/simulator/actors.rs`; verify `cargo build` compiles with new module layout

**Checkpoint**: `cargo build --package ven` compiles with zero errors. `cargo test` passes. No behavior changes yet — SimState still uses old named fields.

---

## Phase 3: User Story 5 — Profile YAML Migration (Priority: P1)

**Goal**: All four profile YAML files load correctly with the typed asset list format. VEN instances can start from migrated profiles.

**Independent Test**: Start any VEN container with a migrated profile; confirm it reaches `/health` without error and `GET /sim` returns asset data.

- [X] T009 Update `VEN/src/profile.rs`: add `AssetConfig` enum with `#[serde(tag = "type", rename_all = "snake_case")]` and variants `Ev(EvConfig)`, `Heater(HeaterConfig)`, `Pv(PvConfig)`, `Battery(BatteryConfig)`, `BaseLoad(BaseLoadConfig)`; replace `DeviceConfig` struct with `Vec<AssetConfig>`; update `Profile` struct field `devices: DeviceConfig` → `assets: Vec<AssetConfig>` (depends on T007 for config types)
- [X] T010 [P] [US5] Migrate `VEN/profiles/ven-1.yaml` to typed asset list: `assets: [{type: ev, id: ev, ...}, {type: pv, id: pv, ...}, {type: battery, id: battery, ...}, {type: base_load, id: base_load, baseline_kw: 0.5}]` — ven-1 has no heater; preserve all numeric values exactly
- [X] T011 [P] [US5] Migrate `VEN/profiles/ven-2.yaml` to typed asset list: `assets: [{type: heater, id: heater, ...}, {type: pv, id: pv, ...}, {type: base_load, id: base_load, baseline_kw: 2.0}]` — ven-2 has no ev or battery
- [X] T012 [P] [US5] Migrate `VEN/profiles/ven-3.yaml` to typed asset list: `assets: [{type: ev, id: ev, ...}, {type: heater, id: heater, ...}, {type: pv, id: pv, ...}, {type: base_load, id: base_load, baseline_kw: 0.8}]` — ven-3 has no battery
- [X] T013 [P] [US5] Migrate `VEN/profiles/test.yaml` to typed asset list: preserve `ev.initial_soc: 0.05` and `battery.capacity_kwh: 10.0` exactly — these values drive the planner FIRM/FLEXIBLE split in BDD tests; also preserve all reactor/simulator/planner/packets sections unchanged

**Checkpoint**: `serde_yaml` can deserialize all four YAML files into `Profile` without error. Confirm by running `cargo test` — profile loading is exercised in unit tests if any exist, else verify via VEN startup.

---

## Phase 4: User Story 1 — Existing Simulator Behavior Preserved (Priority: P1)

**Goal**: SimState uses the generic `Vec<AssetEntry>` model; tick loop and power model produce identical physics output; full BDD suite passes.

**Independent Test**: Run `docker compose -f tests/docker-compose.test.yml run --build --rm test-runner` on Pi4 — all 123 scenarios pass.

- [X] T014 [US1] Add `AssetEntry` and `GridMeter` structs to `VEN/src/simulator/mod.rs` per data-model.md; add `EnergyCounter` import from `simulator/energy.rs` for per-asset use (depends on T007, T009)
- [X] T015 [US1] Refactor `SimState` struct in `VEN/src/simulator/mod.rs`: replace named device fields (`ev`, `heater`, `pv`, `battery`, `base_load_w`, `energy`, `net_power_w`, `import_w`, `export_w`, `voltage_v`, `last_tick`) with `assets: Vec<AssetEntry>`, `grid: GridMeter`, `last_tick: Option<DateTime<Utc>>` (depends on T014)
- [X] T016 [US1] Implement `SimState::from_profile(profile: &Profile)` in `VEN/src/simulator/mod.rs`: iterate `profile.assets`; for each `AssetConfig` variant construct the matching `AssetState`; wrap in `AssetEntry { id, state, setpoint: 0.0, energy: EnergyCounter::default() }`; initialize `grid` to zeroed `GridMeter` (depends on T015)
- [X] T017 [US1] Refactor `SimState::tick(dt_s, setpoints: HashMap<String, f64>, now, overrides)` in `VEN/src/simulator/mod.rs`: build `TickEnvironment` from `now` + `overrides.ambient_temp_c`; for each asset look up setpoint in map (fall back to `asset.state.default_setpoint()`); call `asset.state.update(dt_s, sp, &env)`; update per-asset `EnergyCounter`; apply UserOverrides force fields by asset id; derive `GridMeter` by summing asset power_kw; integrate grid energy (depends on T015, T016)
- [X] T018 [US1] Simplify `VEN/src/simulator/power_model.rs`: replace `compute_net_power(base_load_w, ev_w, heater_w, pv_w)` with `sum_asset_powers(entries: &[AssetEntry]) -> f64` that sums `asset.setpoint` (actual output after tick); keep `voltage_v` random generation; update all callers (depends on T017)
- [X] T019 [US1] Update `VEN/src/simulator/persist.rs`: `SimState` struct shape has changed; verify `save()`/`load()` logic is correct for new struct; confirm `load()` returns `None` on parse failure (old format) and caller reinitializes from profile (no logic change needed — just ensure it still compiles with new `SimState`)
- [X] T020 [US1] Add Setpoints→HashMap bridge in `VEN/src/main.rs` sim tick loop: after `reactor.evaluate()` returns `Setpoints`, convert to `HashMap<String, f64>` keyed by asset id (`"ev"`, `"heater"`, `"pv"`, `"battery"`); after dispatcher overlay on `Setpoints`, re-apply overlay values to the same map; pass `HashMap` to `sim.tick()` (depends on T017)

**Checkpoint**: `cargo build` compiles. Deploy to Pi4 (`git push` → `git pull` on Pi4). Run full BDD suite with `--build`. All 123 scenarios pass. Fix any regressions before proceeding.

---

## Phase 5: User Story 2 — Generic Asset State API (Priority: P2)

**Goal**: `GET /sim` returns the new `{ assets: { "<id>": { power_kw, values } } }` format with no named top-level device fields.

**Independent Test**: `curl http://Pi4-Server:8211/sim | jq '.assets | keys'` returns asset id list; `jq '.ev'` returns null (no top-level named field).

- [X] T021 [US2] Add `AssetSnapshot { power_kw: f64, values: HashMap<String, f64> }` type to `VEN/src/simulator/mod.rs` (depends on T015)
- [X] T022 [US2] Redefine `SimSnapshot` in `VEN/src/simulator/mod.rs` with fields: `ts`, `net_power_w`, `import_w`, `export_w`, `import_kwh`, `export_kwh`, `assets: HashMap<String, AssetSnapshot>` — remove `EvSnapshot`, `HeaterSnapshot`, `PvSnapshot`, `BatterySnapshot` types (depends on T021)
- [X] T023 [US2] Implement `SimState::to_sim_snapshot()` in `VEN/src/simulator/mod.rs`: iterate `self.assets`; for each entry call `asset.state.state_values()`, build `AssetSnapshot { power_kw: last_actual_kw, values }`; collect into HashMap; populate grid totals from `self.grid` (depends on T022)
- [X] T024 [US2] Search `tests/steps/` for any step definitions that reference old `GET /sim` response field paths (e.g., `response["ev"]["soc_pct"]`, `response["heater"]["temp_c"]`); update them to use new path `response["assets"]["ev"]["values"]["soc_pct"]` etc. — **no `.feature` file changes allowed, only step definition `.py` files in `tests/steps/`**

**Checkpoint**: `GET /sim` response matches `contracts/sim-endpoints.md`. BDD simulator scenarios that assert on `/sim` fields pass.

---

## Phase 6: User Story 3 — Control Schema Discovery (Priority: P3)

**Goal**: `GET /sim/schema` returns a map from asset id to list of `ControlDescriptor` for all configured assets.

**Independent Test**: `curl http://Pi4-Server:8211/sim/schema | jq 'keys'` returns asset id list; each entry is a non-empty array of descriptors with `key`, `label`, `kind`, `unit`.

- [X] T025 [US3] Add `GET /sim/schema` route and handler to `VEN/src/main.rs`: handler acquires read lock on sim state; iterates `assets`; calls `asset.state.control_schema()`; collects into `Json(HashMap<String, Vec<ControlDescriptor>>)`; registers route alongside existing `/sim` routes (depends on T007, T017)

**Checkpoint**: `GET /sim/schema` returns non-empty descriptor lists for flexible assets (EV, heater, battery). `base_load` may return empty vec.

---

## Phase 7: User Story 4 — Asset Reset and Config Endpoints (Priority: P3)

**Goal**: Dedicated endpoints replace the three UserOverrides stub fields for initializing asset state.

**Independent Test**: POST to each reset/config endpoint; verify change is reflected in next `GET /sim` response and persists across a tick.

- [X] T026 [US4] Remove `ev_initial_soc: Option<f64>`, `battery_initial_soc: Option<f64>`, `battery_capacity_kwh: Option<f64>` from `UserOverrides` in `VEN/src/state.rs`; fix any compilation errors from removed fields (depends on T015)
- [X] T027 [US4] Add `POST /sim/reset/ev` and `POST /sim/reset/battery` routes + handlers in `VEN/src/main.rs`: each accepts `{ "soc": f64 }`; finds asset by id in sim state; calls `asset.state.reset(HashMap::from([("soc", value)]))` ; validates soc ∈ [0,1]; returns 200 or 404/400; persists `sim_state.json` after update (depends on T026)
- [X] T028 [US4] Add `PUT /sim/config/battery` route + handler in `VEN/src/main.rs`: accepts `{ "capacity_kwh": f64 }`; finds battery asset; calls `asset.state.update_config(HashMap::from([("capacity_kwh", value)]))` ; validates value > 0.0; returns 200 or 404/400; persists `sim_state.json` (depends on T027)
- [X] T029 [US4] Search `tests/steps/` for BDD step definitions that POST `ev_initial_soc`, `battery_initial_soc`, or `battery_capacity_kwh` to `/sim/override`; update those steps to call the new endpoint (`POST /sim/reset/ev`, `POST /sim/reset/battery`, `PUT /sim/config/battery`) — **no `.feature` file changes allowed**

**Checkpoint**: All three new endpoints return 200 for valid input. BDD scenarios that previously used stub fields now pass via new endpoints. `GET /sim/override` schema no longer includes the three removed fields.

---

## Phase 8: Cross-Cutting — History Buffer & Final Verification

**Purpose**: Add the AssetHistoryBuffer data structure (speckit 2 dependency) and run final full BDD verification gate.

- [X] T030 Add `AssetHistoryBuffer` and `AssetTimelinePoint` to `VEN/src/controller/trace.rs`: implement `push(ts, values)` with capacity eviction and NAN fill for missing columns; implement `to_timeline(window)` returning row-oriented `Vec<AssetTimelinePoint>`; add `use std::collections::{HashMap, VecDeque}` and `chrono::DateTime<Utc>` imports (data structure only — no callers yet)
- [X] T031 Deploy to Pi4 and run final BDD verification: `ssh Pi4-Server "cd /srv/docker/openadr_lab && git pull && docker compose -f tests/docker-compose.test.yml run --build --rm test-runner"` — **all 123 scenarios / 801 steps must pass with 0 failures**; fix any remaining regressions

---

## Phase 9: Polish & Documentation

- [X] T032 [P] Record implementation in `docs/history/project_journal.md`: document what was done in each step, any issues encountered (sim_state.json migration, Setpoints bridge, step definition updates), and key learnings
- [X] T033 [P] Update `docs/reference/KEY_LEARNINGS.md` with any hard-won lessons from this refactor (e.g., serde tagged enum gotchas, match exhaustiveness for asset dispatch, sim_state.json migration behavior)

---

## Dependencies & Execution Order

### Phase Dependencies

```
Phase 1 (T001) — module skeleton
  └── Phase 2 (T002–T008) — AssetState interface [T002-T006 parallel]
        └── Phase 3 (T009–T013) — profile.rs + YAML migration [T010-T013 parallel]
              └── Phase 4 (T014–T020) — SimState reform (sequential within phase)
                    └── Phase 5 (T021–T024) — SimSnapshot reform
                          ├── Phase 6 (T025) — GET /sim/schema
                          └── Phase 7 (T026–T029) — reset/config endpoints
                                └── Phase 8 (T030–T031) — buffer + BDD gate
                                      └── Phase 9 (T032–T033) — docs [parallel]
```

### User Story Dependencies

- **US5 (P1) — Profile Migration**: Depends on Phase 2 (AssetConfig types must exist)
- **US1 (P1) — Behavior Preserved**: Depends on US5 (profile.rs must provide Vec<AssetConfig>)
- **US2 (P2) — Generic API**: Depends on US1 (SimState must use Vec<AssetEntry>)
- **US3 (P3) — Schema**: Depends on Phase 2 (control_schema() method must exist)
- **US4 (P3) — Reset/Config**: Depends on US1 (SimState.assets must be iterable by id)

### Within Each Phase

- Phase 2: T002–T006 (five asset files) are fully parallel — different files, same target interface
- Phase 3: T010–T013 (four YAML files) are fully parallel — independent config files
- Phase 4: T014→T015→T016→T017→T018→T019→T020 — strictly sequential (each builds on previous struct/method)
- Phase 5: T021→T022→T023 sequential; T024 can start after T022 (knows new field paths)
- Phase 9: T032–T033 fully parallel — different documentation files

---

## Parallel Execution Examples

### Phase 2: Asset Type Implementation (run together)

```
Agent A: T002 — ev.rs (EvCharger all 8 methods)
Agent B: T003 — heater.rs (Heater all 8 methods)
Agent C: T004 — pv.rs (PvInverter all 8 methods)
Agent D: T005 — battery.rs (Battery all 8 methods)
Agent E: T006 — base_load.rs (BaseLoad all 8 methods)
# Then T007 (wire enum delegation) after all complete
```

### Phase 3: YAML Migration (run together)

```
Agent A: T010 — ven-1.yaml
Agent B: T011 — ven-2.yaml
Agent C: T012 — ven-3.yaml
Agent D: T013 — test.yaml
```

---

## Implementation Strategy

### MVP First (US1 + US5 — Behavior Preserved + Profile Migration)

1. Complete Phase 1: Setup (T001)
2. Complete Phase 2: AssetState interface (T002–T008)
3. Complete Phase 3: Profile migration (T009–T013)
4. Complete Phase 4: SimState reform (T014–T020)
5. **STOP and VALIDATE**: Deploy to Pi4, run BDD suite — all 123 scenarios must pass
6. If passing: refactor is structurally complete; US2–US4 are additive

### Incremental Delivery

1. Phases 1–4 → Foundation + behavior preservation (MVP gate: BDD passes)
2. Phase 5 → Generic `/sim` response (US2)
3. Phase 6 → Schema endpoint (US3)
4. Phase 7 → Reset/config endpoints (US4)
5. Phase 8 → Final BDD gate (all 35 tasks complete)

---

## Notes

- **Never modify `.feature` files** — only step definition `.py` files in `tests/steps/` when response shapes change
- **Always use `--build`** when running test-runner after any source change (image bakes source at build time)
- **Test YAML migration locally** with `cargo build` before deploying — serde_yaml parse errors are caught at compile time via `#[cfg(test)]` profile load tests if present, else at VEN startup
- **Setpoints bridge is temporary** — it is removed in speckit 2 when reactor is refactored
- **sim_state.json on Pi4** will fail to deserialize (old format) on first deploy — this is expected; VEN reinitializes from profile defaults and logs a warning
- **T031 is the hard acceptance gate** — no task after T030 is "done" until all 123 BDD scenarios pass
