# Tasks: Reporter Multi-Interval Resampling (RF-05e)

**Input**: Design documents from `/specs/012-reporter-resampling/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md

**Tests**: BDD scenarios required per Constitution Principle II (BDD-First Testing).

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

---

## Phase 1: Foundational (Blocking Prerequisites)

**Purpose**: Helper function and conversion logic that all user stories depend on

- [x] T001 Add `history_to_timeseries()` helper function in `VEN/src/controller/reporter.rs` — extracts a named column from `AssetHistoryBuffer` into a `TimeSeries`, skipping NaN values. Parameters: buffer ref, column name (`&str`), `Interpolation` mode, optional time window. Returns `TimeSeries`.
- [x] T002 Add unit tests for `history_to_timeseries()` in `VEN/src/controller/reporter.rs` — test cases: (a) extract `power_kw` column from buffer with 3 rows → 3-sample Step TimeSeries, (b) extract column with NaN gaps → NaN rows excluded, (c) extract from empty buffer → empty TimeSeries, (d) extract nonexistent column → empty TimeSeries.

**Checkpoint**: `history_to_timeseries()` tested and working — user story implementation can begin

---

## Phase 2: User Story 1+4 — Multi-Interval Reports + Obligation Plumbing (Priority: P1) MVP

**Goal**: Build multi-interval measurement reports using obligation interval duration, and wire obligation fulfillment to actual report submission. Stories 1 and 4 are combined because obligation plumbing (US4) is a prerequisite for multi-interval reports (US1).

**Independent Test**: Trigger a measurement report for an event with a 15-minute reportDescriptor and 1 hour of history; verify 4 interval rows with correct time-weighted mean values.

### BDD Scenarios

- [x] T003 [P] [US1] Write BDD feature file `tests/features/reporter_resampling.feature` with scenarios: (a) multi-interval report with 15-min obligation and 1h history produces 4 intervals, (b) 1-hour obligation with 2h history produces 2 intervals, (c) event without reportDescriptor falls back to single-interval snapshot, (d) each interval has sequential `id` (0..N) and `intervalPeriod` with `start` and `duration`.
- [x] T004 [P] [US1] Write step definitions in `tests/steps/reporter_steps.py` — steps to: set up VEN with known asset history, create event with reportDescriptor specifying interval duration, trigger report generation, verify interval count and payload values in submitted report.

### Implementation

- [x] T005 [US1] Add `build_measurement_report_for_obligation()` public function in `VEN/src/controller/reporter.rs` — accepts `&OadrReportObligation`, `&HashMap<String, AssetHistoryBuffer>`, `ven_name: &str`. Internally: (a) compute net site power TimeSeries by summing all assets' `power_kw` columns, (b) call `resample_uniform(Duration::seconds(obligation.interval_duration_s))`, (c) build report JSON with one interval entry per resampled bucket, each with sequential `id` and `intervalPeriod`, (d) include OPERATING_STATE payload per interval. Returns `Option<Value>`.
- [x] T006 [US1] Add unit tests for `build_measurement_report_for_obligation()` in `VEN/src/controller/reporter.rs` — test with mock obligation (interval_duration_s=900) and mock asset history (60 rows at 1-min intervals with known power values). Verify: correct interval count, correct time-weighted mean per bucket, correct `intervalPeriod` timestamps, sequential `id` values.
- [x] T007 [US1] Wire obligation fulfillment loop in `VEN/src/main.rs` (~line 496-514) — replace stub with: (a) get `controller_trace()` for asset history, (b) call `build_measurement_report_for_obligation(&ob, &trace.asset_history, &ven_name)`, (c) submit report via `vtn.upsert_report(report)`, (d) mark obligation fulfilled only after successful submission. Add `vtn` client clone to the spawned task's captures.

**Checkpoint**: Multi-interval reports generated and submitted for events with reportDescriptors. Events without reportDescriptors still use timer-driven single-snapshot path.

---

## Phase 3: User Story 2 — Per-Asset-Type Conversion (Priority: P1)

**Goal**: Correctly handle different quantity types (power vs. SoC) with appropriate interpolation and aggregation modes.

**Independent Test**: Build a report for an event with STORAGE_CHARGE_LEVEL obligation; verify SoC values are point-in-time samples at interval boundaries, not time-weighted means.

### BDD Scenarios

- [x] T008 [US2] Add BDD scenario to `tests/features/reporter_resampling.feature` — EV asset with known SoC trajectory (0.2 → 0.8 over 1h), 15-min obligation interval: verify STORAGE_CHARGE_LEVEL payloads contain point-in-time SoC values at each interval end (not averaged values).

### Implementation

- [x] T009 [US2] Extend `build_measurement_report_for_obligation()` in `VEN/src/controller/reporter.rs` to handle STORAGE_CHARGE_LEVEL obligations — when `obligation.payload_type == "STORAGE_CHARGE_STATE"` or `"STORAGE_CHARGE_LEVEL"`: extract `soc` column from EV asset history as Step TimeSeries, compute interval-end timestamps, call `resample_to_grid()` at those timestamps, emit one STORAGE_CHARGE_LEVEL payload per interval with `format!("{:.1}", soc * 100.0)`.
- [x] T010 [US2] Add unit test in `VEN/src/controller/reporter.rs` for SoC-based obligation report — mock EV history with linearly rising SoC, verify `resample_to_grid()` produces correct point-in-time values at interval ends.

**Checkpoint**: Reports correctly distinguish power (time-weighted mean) from SoC (point-in-time) quantities.

---

## Phase 4: User Story 3 — Import/Export Power Split (Priority: P2)

**Goal**: Correctly split net power into import and export components per resampled interval.

**Independent Test**: Build reports for IMPORT_CAPACITY_LIMIT and EXPORT_CAPACITY_LIMIT events with mixed import/export history; verify per-interval directional values.

### BDD Scenarios

- [x] T011 [US3] Add BDD scenarios to `tests/features/reporter_resampling.feature` — (a) site imports 3kW in first 15min, exports 2kW in second: IMPORT report shows 3000W then 0W, (b) site exports throughout: EXPORT report shows absolute export per interval.

### Implementation

- [x] T012 [US3] Extend `build_measurement_report_for_obligation()` in `VEN/src/controller/reporter.rs` to handle directional split — after resampling net site power: for IMPORT_CAPACITY_LIMIT, clamp each bucket to `max(0, value_w)`; for EXPORT_CAPACITY_LIMIT, clamp each bucket to `max(0, -value_w)`. Apply clamping per-interval in the payload generation loop.
- [x] T013 [US3] Add unit test in `VEN/src/controller/reporter.rs` for import/export split — mock history with alternating positive/negative power_kw values, verify IMPORT report shows only positive buckets and EXPORT report shows only absolute negative buckets.

**Checkpoint**: Directional power reports are correct per interval.

---

## Phase 5: Polish & Cross-Cutting Concerns

**Purpose**: Backward compatibility, documentation, and integration validation

- [x] T014 Verify backward compatibility — run all existing BDD report scenarios (`tests/features/` files that submit or verify reports) and confirm zero regressions. Fix any breakage.
- [x] T015 Update project journal at `docs/history/project_journal.md` — document RF-05e implementation: what changed, key decisions (two report paths, SoC point-in-time vs power TWM), issues encountered.
- [x] T016 Update `docs/reference/KEY_LEARNINGS.md` if any new learnings emerge during implementation.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Foundational)**: No dependencies — can start immediately
- **Phase 2 (US1+US4)**: Depends on Phase 1 (needs `history_to_timeseries`)
- **Phase 3 (US2)**: Depends on Phase 2 (extends `build_measurement_report_for_obligation`)
- **Phase 4 (US3)**: Depends on Phase 2 (extends `build_measurement_report_for_obligation`). Can run in parallel with Phase 3.
- **Phase 5 (Polish)**: Depends on Phases 2-4

### Within Each Phase

- BDD scenarios (T003/T004) are written first and must FAIL before implementation
- Unit tests (T006, T010, T013) can be written alongside implementation
- Call-site wiring (T007) depends on the function it calls (T005)

### Parallel Opportunities

- T003 and T004 can run in parallel (different files)
- Phase 3 (US2) and Phase 4 (US3) can run in parallel after Phase 2 completes (both extend the same function but in different code paths)

---

## Implementation Strategy

### MVP First (Phase 1 + Phase 2)

1. Complete Phase 1: `history_to_timeseries()` helper + tests
2. Complete Phase 2: Multi-interval report builder + obligation wiring
3. **STOP and VALIDATE**: Run BDD scenarios — events with reportDescriptors produce multi-interval reports, events without produce single-interval (backward compat)
4. This alone delivers the core value of RF-05e

### Incremental Delivery

1. Phase 1 + Phase 2 → MVP: multi-interval power reports
2. + Phase 3 → Add SoC point-in-time support
3. + Phase 4 → Add directional import/export split
4. + Phase 5 → Documentation and regression validation

---

## Notes

- All implementation is in a single file (`reporter.rs`) plus one call-site change (`main.rs`)
- Existing `build_measurement_report()` and `build_measurement_reports_for_active_events()` remain untouched for backward compatibility
- The obligation stub loop in `main.rs:496-514` becomes the real obligation-driven report path
- Timer-driven path (`main.rs:460-480`) continues unchanged as fallback
