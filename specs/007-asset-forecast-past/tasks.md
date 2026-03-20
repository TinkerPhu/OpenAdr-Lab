# Tasks: Asset Interface — forecast() and past()

**Input**: Design documents from `/specs/007-asset-forecast-past/`
**Branch**: `007-asset-forecast-past`
**Spec**: [spec.md](spec.md) | **Plan**: [plan.md](plan.md) | **Data model**: [data-model.md](data-model.md) | **Contracts**: [contracts/asset_interface.md](contracts/asset_interface.md)

## Format: `[ID] [P?] [Story?] Description`

- **[P]**: Can run in parallel (different files, no shared dependencies)
- **[Story]**: Which user story this task belongs to (US1 / US2 / US3)

---

## Phase 1: Setup

**Purpose**: Create the `common/` module that all user stories depend on.

- [x] T001 Create `VEN/src/common/mod.rs` with `Interpolation`, `Quantity`, `Unit`, and `QuantitySeries` types as specified in `data-model.md` — derive `Debug`, `Clone` on all types
- [x] T002 Add `mod common;` to `VEN/src/main.rs` and add `use crate::common::*;` where needed; confirm `cargo check` passes with zero errors

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: BDD feature files (Constitution II: tests before code) and dispatch stubs that all three user stories build on. No user story implementation can begin until this phase is complete.

⚠️ **CRITICAL**: Write BDD files first, run them, confirm they FAIL. Only then proceed to implementation.

- [x] T003 [P] Write `tests/features/asset_forecast.feature` — BDD scenarios for `forecast(timespan)` covering: PV at noon (positive power), PV at night (zero), battery at 80% SoC, EV with no session (zero), base-load constant, heater thermal decay, zero timespan returns empty series, mandatory boundary point at `now + timespan`
- [x] T004 [P] Write `tests/features/asset_history.feature` — BDD scenarios for `past(timespan)`: PV 30-min history returns samples, partial buffer (asset just started) returns available data only, empty buffer returns empty series, mandatory boundary point at `now − timespan`
- [x] T005 Add `forecast(timespan: Duration) -> QuantitySeries` and `past(timespan: Duration, history: &AssetHistoryBuffer) -> QuantitySeries` dispatch arms to `impl AssetState` in `VEN/src/simulator/assets/mod.rs`; remove `predict()`; stub implementations return empty `QuantitySeries` so the codebase compiles; fix all call sites of `predict()` to compile
- [x] T006 Run BDD suite on Pi4 for the two new feature files and confirm all new scenarios **FAIL** (red phase); fix any step-definition compile errors without touching implementation

**Checkpoint**: BDD scenarios exist and fail. Dispatch stubs compile. Implementation can now begin.

---

## Phase 3: User Story 1 — Planner Uses Per-Asset Forecasts (P1) 🎯 MVP

**Goal**: Remove `pv_forecast()` from the planner; every asset provides its own forecast; planner receives a pre-computed forecast map.

**Independent Test**: Implement PV forecast only (T007), wire planner (T012–T013), run existing controller BDD suite — plan must still be produced correctly with PV generation correctly modelled; `pv_forecast()` must not exist in `planner.rs`.

- [x] T007 [P] [US1] Implement `forecast(timespan)` on `PvInverter` in `VEN/src/simulator/assets/pv.rs`: sinusoidal irradiation model `sin(π × (hour − 6) / 12)` for hours 6–18 clamped to 0, power = `−rated_kw × irradiance` (negative = export), 1 sample/minute, mandatory boundary point at `now + timespan`; `quantity = Power`, `unit = Kilowatt`, `interpolation = Linear`
- [x] T008 [P] [US1] Implement `forecast(timespan)` on `Battery` in `VEN/src/simulator/assets/battery.rs`: project SOC trajectory at current setpoint (constant power = `setpoint.clamp(−max_discharge_kw, max_charge_kw)`); power drops to 0 when SoC hits min or max within the horizon; 1 sample/minute; boundary point; `interpolation = Linear`
- [x] T009 [P] [US1] Implement `forecast(timespan)` on `EvCharger` in `VEN/src/simulator/assets/ev.rs`: two samples only — `(now, setpoint_or_zero)` and boundary point at `now + timespan` with same value; `interpolation = Step`
- [x] T010 [P] [US1] Implement `forecast(timespan)` on `Heater` in `VEN/src/simulator/assets/heater.rs`: thermal decay toward setpoint using existing thermal model coefficients; 1 sample/minute; boundary point; `interpolation = Linear`
- [x] T011 [P] [US1] Implement `forecast(timespan)` on `BaseLoad` in `VEN/src/simulator/assets/base_load.rs`: two samples — `(now, baseline_kw)` and boundary point at `now + timespan` with same value; `interpolation = Step`
- [x] T012 [US1] In `VEN/src/controller/planner.rs`: remove `pv_forecast()` function; add `asset_forecasts: &HashMap<String, QuantitySeries>` parameter to `run_planner()` and `build_grid()`; replace `pv_forecast(profile, start)` call with `nearest_value(asset_forecasts.get("pv"), start)`; add private `nearest_value(series: Option<&QuantitySeries>, ts: DateTime<Utc>) -> f64` helper (Step: last-value-at-or-before; Linear: nearest-neighbour; absent/empty: 0.0)
- [x] T013 [US1] In `VEN/src/main.rs`: before each `run_planner()` call, compute `asset_forecasts: HashMap<String, QuantitySeries>` by calling `entry.state.forecast(planning_horizon_duration)` for each asset in `sim_state.assets`; pass map to `run_planner()`
- [x] T014 [US1] Run `cargo test` locally; fix any remaining compile errors; then on Pi4 run full BDD suite (`docker compose -f tests/docker-compose.test.yml run --build --rm test-runner`) — all existing scenarios must pass plus `asset_forecast.feature` scenarios must now pass

**Checkpoint**: PV forecast drives the planner. `pv_forecast()` is gone. All existing BDD tests green.

---

## Phase 4: User Story 2 — UI Timeline Uses Asset History (P2)

**Goal**: Each asset can return its own historical power data via `past(timespan, history)`; the timeline endpoint sources data through this method.

**Independent Test**: Call `GET /timeline/pv?hours_back=0.5` — response must contain samples from the last 30 minutes with no NaN values; same call for battery; simulated vs. measured assets must produce the same response shape.

- [x] T015 [US2] Add `past_from_buffer(timespan: Duration, history: &AssetHistoryBuffer, interpolation: Interpolation) -> QuantitySeries` shared helper to `VEN/src/simulator/assets/mod.rs`: slice buffer to `[now − timespan, now]`, extract `power_kw` column (drop NaN rows), prepend boundary point at `now − timespan` using declared interpolation mode, return `QuantitySeries { samples, quantity: Power, unit: Kilowatt, interpolation }`
- [x] T016 [P] [US2] Implement `past(timespan, history)` on `PvInverter` in `VEN/src/simulator/assets/pv.rs`: delegate to `past_from_buffer` with `interpolation = Linear`
- [x] T017 [P] [US2] Implement `past(timespan, history)` on `Battery` in `VEN/src/simulator/assets/battery.rs`: delegate to `past_from_buffer` with `interpolation = Linear`
- [x] T018 [P] [US2] Implement `past(timespan, history)` on `EvCharger` in `VEN/src/simulator/assets/ev.rs`: delegate to `past_from_buffer` with `interpolation = Step`
- [x] T019 [P] [US2] Implement `past(timespan, history)` on `Heater` in `VEN/src/simulator/assets/heater.rs`: delegate to `past_from_buffer` with `interpolation = Linear`
- [x] T020 [P] [US2] Implement `past(timespan, history)` on `BaseLoad` in `VEN/src/simulator/assets/base_load.rs`: delegate to `past_from_buffer` with `interpolation = Step`
- [x] T021 [US2] Wire `past()` into the timeline handler in `VEN/src/main.rs`: for each asset in `sim_state.assets`, replace direct `trace.asset_history_for(id)` slice with `asset.past(window_duration, history_buf)` and return the `QuantitySeries` samples; keep `"grid"` virtual asset using existing direct buffer access (no `AssetState` for grid)
- [ ] T022 [US2] On Pi4 run BDD suite with `--build`; `asset_history.feature` scenarios must now pass alongside all existing scenarios

**Checkpoint**: Timeline sourced via `past()`. US1 and US2 both independently green.

---

## Phase 5: User Story 3 — Simulated and Measured Assets Are Interchangeable (P3)

**Goal**: Confirm the interface contract holds — replacing a simulated asset with a measured stub requires zero changes in planner, dispatcher, or reporter.

**Independent Test**: Swap `PvInverter` (simulated) for `MeasuredPv` stub in the test profile; run full BDD suite; zero source changes outside the asset file itself.

- [ ] T023 [US3] Add `MeasuredPv` struct to `VEN/src/simulator/assets/pv.rs` (or a `#[cfg(test)]` module): implements `forecast(timespan)` returning a flat `QuantitySeries` at current power (Linear interpolation), and `past(timespan, history)` delegating to `past_from_buffer`; add a `AssetState::MeasuredPv(MeasuredPv)` variant (or use feature flag) so it can be loaded from a test profile
- [ ] T024 [US3] Create `VEN/profiles/measured_test.yaml` with `MeasuredPv` as the PV asset; run the full BDD suite on Pi4 pointing to this profile; confirm all scenarios pass with zero changes to `planner.rs`, `dispatcher.rs`, or `reporter.rs`
- [ ] T025 [US3] Remove `MeasuredPv` variant and `measured_test.yaml` — interchangeability verified; document outcome in `docs/history/project_journal.md` under RF-01

**Checkpoint**: Interface contract verified. Measured assets can replace simulated ones without controller changes.

---

## Phase 6: Polish & Cross-Cutting Concerns

- [x] T026 [P] Add `cargo test` unit tests for edge cases in `VEN/src/simulator/assets/` test modules: zero timespan returns empty series, PV at night returns all-zero samples, battery at full SoC with charge setpoint returns zero power, EV with no session returns zero, boundary point timestamp equals `now + timespan` exactly
- [x] T027 [P] Add `cargo test` unit tests for `QuantitySeries` invariants in `VEN/src/common/mod.rs`: samples are ascending, boundary point present on non-empty series, `quantity`/`unit`/`interpolation` match expected values per asset type
- [ ] T028 Run full BDD suite on Pi4 across all 27+ features to confirm zero regressions from this refactoring
- [ ] T029 Write journal entry in `docs/history/project_journal.md`: what was done, pv_forecast() removal rationale, QuantitySeries design decisions, key learnings

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Setup)**: No dependencies — start immediately
- **Phase 2 (Foundational)**: Depends on Phase 1 (T001–T002 must be done before T005 can compile)
- **Phase 3 (US1)**: Depends on Phase 2 complete — T007–T011 can run in parallel; T012 depends on T007; T013 depends on T012; T014 depends on T007–T013
- **Phase 4 (US2)**: Depends on Phase 2; T015 must precede T016–T020; T021 depends on T015–T020; T022 depends on T021
- **Phase 5 (US3)**: Depends on Phase 3 and Phase 4 both complete
- **Phase 6 (Polish)**: Depends on Phase 5

### User Story Dependencies

- **US1**: No dependency on US2 or US3
- **US2**: No dependency on US1 or US3 (can develop in parallel with US1 after Phase 2)
- **US3**: Depends on US1 and US2 being complete (needs both forecast and past working)

---

## Parallel Opportunities

```
# Phase 1 — sequential (T002 depends on T001)
T001 → T002

# Phase 2 — BDD files in parallel, then stubs
T003 ∥ T004  →  T005  →  T006

# Phase 3 — five forecast implementations in parallel
T007 ∥ T008 ∥ T009 ∥ T010 ∥ T011  →  T012  →  T013  →  T014

# Phase 4 — one helper, then five past() in parallel
T015  →  T016 ∥ T017 ∥ T018 ∥ T019 ∥ T020  →  T021  →  T022

# Phase 3 and Phase 4 can run in parallel after Phase 2 if two developers
(T007–T014) ∥ (T015–T022)

# Phase 6 — unit test tasks in parallel
T026 ∥ T027  →  T028  →  T029
```

---

## Implementation Strategy

### MVP (User Story 1 only)

1. Phase 1: Create `common/` module
2. Phase 2: BDD files + dispatch stubs (confirm red)
3. Phase 3: PV forecast first (T007) → planner wiring (T012–T013) → remaining assets (T008–T011) → green (T014)
4. **STOP and VALIDATE**: Planner runs without `pv_forecast()`, all controller BDD tests green

### Incremental Delivery

1. Setup + Foundational → compiles, BDD red
2. US1 complete → planner using asset forecasts, BDD green
3. US2 complete → timeline sourced via `past()`, BDD green
4. US3 complete → interface contract verified
5. Polish → full regression + docs

### Key constraints (from Constitution)

- BDD `.feature` files MUST be written and confirmed failing before any implementation code (T003–T004 before T007)
- Always pass `--build` when running Docker test suite — VEN Rust source is baked into image
- SQLx cache does not need regeneration — no SQL changes in this refactoring
