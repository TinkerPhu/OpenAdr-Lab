# Tasks: Backend Adoption of TimeSeries Resampling

**Input**: Design documents from `/specs/009-backend-timeseries-adoption/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/planner-interface.md

**Tests**: Unit tests are included — the spec explicitly requires new unit tests (SC-005, SC-006) and regression verification (SC-001, SC-002).

**Organization**: Tasks grouped by user story. US3 (tariff conversion) is a foundational prerequisite for US1 and US2, so it is in Phase 2 despite being P2 in the spec.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

---

## Phase 1: Setup

**Purpose**: No project initialization needed — this is a refactor within existing code. Phase 1 is empty.

---

## Phase 2: Foundational — TariffTimeSeries struct + conversion (US3)

**Purpose**: US3 (tariff conversion at interface boundary) must be complete before US1 or US2 can consume `TariffTimeSeries`. This phase creates the new type and its conversion function.

**Goal**: `TariffSnapshot` lists are convertible to three independent `TimeSeries` (import, export, CO2) with Step interpolation.

**Independent Test**: Unit tests verify conversion from `Vec<TariffSnapshot>` to `TariffTimeSeries` for normal, None-gap, empty, unsorted, and duplicate inputs.

- [x] T001 [US3] Add `TariffTimeSeries` struct with three `TimeSeries` fields (`import_eur_kwh`, `export_eur_kwh`, `co2_g_kwh`) and `from_snapshots()` constructor in `VEN/src/entities/tariff_snapshot.rs`. Conversion rule: for each `TariffSnapshot`, emit `(interval_start, value)` into the corresponding series only if the field is `Some`. Sort each series by timestamp; last-write-wins for duplicates. Add `use crate::common::{TimeSeries, Interpolation}` import.
- [x] T002 [US3] Add unit tests for `TariffTimeSeries::from_snapshots()` in `VEN/src/entities/tariff_snapshot.rs`: (a) normal case with all three quantities present, (b) `None` gaps — CO2 missing while import/export present, (c) empty input → all three series empty, (d) unsorted input → output sorted, (e) duplicate timestamps → last-write-wins.
- [x] T003 [US3] Update caller in `VEN/src/main.rs` planning loop (~line 550): convert `Vec<TariffSnapshot>` to `TariffTimeSeries` via `TariffTimeSeries::from_snapshots(&rates)` before calling `run_planner()`. Pass `&tariff_ts` instead of `&rates`.

**Checkpoint**: `TariffTimeSeries` struct exists, conversion is tested, caller compiles (planner signature updated in next phase).

---

## Phase 3: User Story 1 — Planner tariff lookups use pre-resampled series (Priority: P1)

**Goal**: The planner's slot loop reads tariff values from pre-resampled `HashMap<i64, f64>` lookups instead of per-slot `tariff_*_at()` scans. Tariffs spanning slot boundaries are correctly time-weighted.

**Independent Test**: Unit tests verify boundary-aligned tariffs produce identical results (regression), mid-slot tariff changes produce correct time-weighted averages, and gaps fall back to defaults.

- [x] T004 [US1] Change `run_planner()` signature in `VEN/src/controller/planner.rs`: replace `rates: &[TariffSnapshot]` with `tariffs: &TariffTimeSeries`. Update `build_grid()` signature accordingly. Add `use crate::entities::tariff_snapshot::TariffTimeSeries` import.
- [x] T005 [US1] In `build_grid()` in `VEN/src/controller/planner.rs`: before the slot loop, resample each tariff series to slot width. Build three `HashMap<i64, f64>` (keyed by epoch seconds) from the resampled samples: `tariffs.import_eur_kwh.resample_uniform(slot_duration)`, same for `export_eur_kwh` and `co2_g_kwh`. Use `chrono::Duration::seconds(step_s as i64)` as the slot duration.
- [x] T006 [US1] In the slot loop in `build_grid()` in `VEN/src/controller/planner.rs`: replace `tariff_import_at(rates, start)` with `import_map.get(&start.timestamp()).copied()`, same for export and CO2. Keep `.unwrap_or(DEFAULT_*)` fallback. Update `rate_estimated` flag: set to `true` if all three tariff series have empty samples (`.samples.is_empty()`).
- [x] T007 [US1] Remove the three helper functions `tariff_import_at()`, `tariff_export_at()`, `tariff_co2_at()` from `VEN/src/controller/planner.rs` (~lines 545-564). Verify no other callers exist.
- [x] T008 [US1] Add unit tests for tariff resampling in planner in `VEN/src/controller/planner.rs`: (a) boundary-aligned tariffs produce identical slot values as the old implementation, (b) mid-slot tariff change (import changes from 0.20 to 0.15 at minute 3 of a 5-min slot) → slot gets time-weighted average 0.18, (c) empty tariff series → all slots get default values with `rate_estimated: true`, (d) single-sample tariff → all slots get that value (Step LOCF), (e) gaps in tariff coverage → affected slots fall back to defaults.
- [x] T009 [US1] Run `cargo test` in `VEN/` to verify all existing planner tests still pass (regression SC-002).

**Checkpoint**: Tariff lookups fully migrated. Old helpers removed. `cargo test` green.

---

## Phase 4: User Story 2 — Asset forecast lookups use resampled series (Priority: P1)

**Goal**: The planner's slot loop reads asset forecast values from pre-resampled `HashMap<i64, f64>` lookups instead of per-slot `nearest_value()` calls. All interpolation modes handled correctly by `resample_uniform()`.

**Independent Test**: Unit tests verify PV Linear forecasts produce time-weighted means, Step forecasts produce LOCF values, and empty/missing forecasts default to 0.0.

- [x] T010 [US2] In `build_grid()` in `VEN/src/controller/planner.rs`: before the slot loop, resample each asset forecast to slot width. For each entry in `asset_forecasts`, call `.resample_uniform(slot_duration)` and build a `HashMap<String, HashMap<i64, f64>>` keyed by asset ID then epoch seconds.
- [x] T011 [US2] In the slot loop in `build_grid()` in `VEN/src/controller/planner.rs`: replace `nearest_value(asset_forecasts.get("pv"), start)` with lookup from the resampled forecast map: `forecast_maps.get("pv").and_then(|m| m.get(&start.timestamp()).copied()).unwrap_or(0.0)`. Apply same pattern to any other asset forecast lookups.
- [x] T012 [US2] Remove `nearest_value()` helper function from `VEN/src/controller/planner.rs` (~lines 572-600). Verify no other callers exist.
- [x] T013 [US2] Add unit tests for asset forecast resampling in planner in `VEN/src/controller/planner.rs`: (a) PV with Linear interpolation at non-slot-aligned timestamps → slot gets time-weighted mean, (b) EV with Step interpolation → slot gets LOCF value, (c) empty forecast → slot defaults to 0.0, (d) missing asset key in forecast map → defaults to 0.0.
- [x] T014 [US2] Run `cargo test` in `VEN/` to verify all existing tests still pass after forecast migration.

**Checkpoint**: All ad-hoc lookup functions removed (SC-004). `cargo test` green.

---

## Phase 5: User Story 4 — Report generation uses resampled history (Priority: P3)

**Goal**: The reporter resamples asset history to obligation intervals using `resample_uniform()`, producing one report row per interval instead of a single latest-snapshot.

**Independent Test**: Unit tests verify multi-interval reports with correctly aggregated values and sparse-data handling.

- [SKIPPED] T015 [US4] Deferred to RF-05e — reporter resampling requires deeper refactor (see BACKLOG.md).
- [SKIPPED] T016 [US4] Deferred to RF-05e.
- [SKIPPED] T017 [US4] Deferred to RF-05e.

**Checkpoint**: Reporter uses resampled history (SC-006). `cargo test` green.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Integration testing and final verification across all stories.

- [x] T018 Build VEN Docker image on Pi4-Server: `ssh Pi4-Server "cd /srv/docker/openadr_lab && git pull && docker compose -f tests/docker-compose.test.yml run --build --rm test-runner"` — run full BDD suite (143+ scenarios, 895+ steps). All must pass (SC-001). ✓ 36 features, 173 scenarios, 1010 steps — all passed.
- [x] T019 Verify planner output for boundary-aligned tariffs is bit-for-bit identical to previous implementation by comparing BDD scenario outputs that check plan slot values (SC-002). ✓ All plan-related scenarios passed unchanged.
- [x] T020 Update project journal at `docs/history/project_journal.md` with RF-05b implementation summary: what changed, why, key learnings.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Setup)**: Empty — no setup needed.
- **Phase 2 (Foundational — US3)**: No dependencies. Creates `TariffTimeSeries` struct + conversion. BLOCKS Phase 3.
- **Phase 3 (US1 — Tariff resampling)**: Depends on Phase 2 (needs `TariffTimeSeries` type and caller update).
- **Phase 4 (US2 — Forecast resampling)**: Depends on Phase 3 (shares `build_grid()` modifications). Could technically run in parallel with Phase 3 since they touch different parts of the slot loop, but sequential is safer to avoid merge conflicts.
- **Phase 5 (US4 — Reporter)**: Independent of Phases 3-4. Can run in parallel after Phase 2.
- **Phase 6 (Polish)**: Depends on all previous phases.

### User Story Dependencies

- **US3 (P2, Foundational)**: No dependencies — creates the type consumed by US1/US2.
- **US1 (P1)**: Depends on US3 (needs `TariffTimeSeries` parameter).
- **US2 (P1)**: Depends on US3 (shares planner modifications with US1). Sequential after US1 recommended.
- **US4 (P3)**: Independent — only touches `reporter.rs`. Can run in parallel with US1/US2 after Phase 2.

### Within Each User Story

- Signature/struct changes before slot loop changes
- Slot loop changes before helper removal
- Helper removal before unit tests (tests verify the new path, not the old one)
- `cargo test` after each phase

### Parallel Opportunities

- **T001 and T002**: Sequential (T002 tests T001's output)
- **T004–T007**: Sequential within US1 (all modify `planner.rs`)
- **T010–T012**: Sequential within US2 (all modify `planner.rs`)
- **T015–T016**: Sequential within US4 (both modify `reporter.rs`)
- **Phase 4 (US2) and Phase 5 (US4)**: Could run in parallel (different files: `planner.rs` vs `reporter.rs`)

---

## Parallel Example: After Phase 2

```text
# After TariffTimeSeries is in place, these can run in parallel:
Developer A: Phase 3 (US1) — tariff resampling in planner.rs
Developer B: Phase 5 (US4) — reporter resampling in reporter.rs

# Then Phase 4 (US2) follows US1 since both touch planner.rs
```

---

## Implementation Strategy

### MVP First (US3 + US1 Only)

1. Complete Phase 2: TariffTimeSeries struct + conversion (US3)
2. Complete Phase 3: Tariff resampling in planner (US1)
3. **STOP and VALIDATE**: `cargo test` — all existing tests pass, new tariff tests pass
4. This alone delivers the core value: correct time-weighted tariffs, no O(n) scans

### Incremental Delivery

1. Phase 2 (US3) → TariffTimeSeries type ready
2. Phase 3 (US1) → Tariff lookups migrated → `cargo test` → **MVP complete**
3. Phase 4 (US2) → Forecast lookups migrated → `cargo test` → All ad-hoc helpers removed (SC-004)
4. Phase 5 (US4) → Reporter migrated → `cargo test` → Full feature complete
5. Phase 6 → Full BDD suite on Pi4 → SC-001 verified → Ready to merge

---

## Notes

- All tasks modify existing files — no new files created
- Naming convention: `tariff_eur_per_kwh` suffix per CLAUDE.md naming rules (but struct fields use `_eur_kwh` to match existing `TariffSnapshot` field names for consistency)
- The `TariffTimeSeries` struct lives in `entities/tariff_snapshot.rs` alongside the existing `TariffSnapshot` to keep related types together
- `resample_uniform()` returns a `TimeSeries` with `.samples: Vec<(DateTime<Utc>, f64)>` — convert to `HashMap<i64, f64>` via `.samples.iter().map(|(ts, v)| (ts.timestamp(), *v)).collect()`
