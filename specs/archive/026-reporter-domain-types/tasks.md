# Tasks: Reporter — Domain-Side Snapshot Types

**Input**: Design documents from `/specs/026-reporter-domain-types/`
**Prerequisites**: plan.md ✓, spec.md ✓, research.md ✓, data-model.md ✓

**Organization**: Tasks grouped by user story. US1 and US2 are both P1 but sequential (US1 must compile before US2 callsites can be validated).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks in this phase)
- **[Story]**: Which user story this task belongs to (US1, US2, US3)

---

## Phase 1: Setup — Baseline Verification

**Purpose**: Record the before-state before touching any code.

- [x] T001 Run `grep "use crate::simulator\|use crate::assets" VEN/src/controller/reporter.rs` and confirm it currently returns matches (2 lines). Run `wsl cargo test -p ven` and confirm all tests currently pass (establishes baseline count).

---

## Phase 2: Foundational — New Domain Type

**Purpose**: Add `AssetReportSample` and `SimSnapshot` import to `reporter.rs`. Nothing is removed yet; this only adds new code. **Blocks US1 and US2.**

- [x] T002 In `VEN/src/controller/reporter.rs` — add `use crate::controller::simulator_port::SimSnapshot;` to the existing import block. Then add `pub struct AssetReportSample { pub ts: DateTime<Utc>, pub power_kw: f64, pub soc: Option<f64> }` after the import block, before the first helper function (after the closing `use` statements, as per project style). Verify `wsl cargo check -p ven` still passes.

**Checkpoint**: `AssetReportSample` is in scope. Existing code unchanged. ✓

---

## Phase 3: User Story 1 — Domain-Pure Reporter (Priority: P1) 🎯 MVP

**Goal**: Every function in `reporter.rs` accepts only domain-ring types. All infra imports removed. New `#[cfg(test)]` tests verify functions can be called without `SimState`.

**Independent Test**: `wsl cargo test -p ven controller::reporter` — all tests pass, including the two new SC-004/SC-005 tests.

### Implementation for User Story 1

- [x] T003 [US1] In `VEN/src/controller/reporter.rs` — rename `points_to_power_ts(points: &[HistoryPoint], interpolation: Interpolation) -> TimeSeries` to `samples_to_power_ts(samples: &[AssetReportSample], interpolation: Interpolation) -> TimeSeries`. Update the body: `let samples_out: Vec<(DateTime<Utc>, f64)> = samples.iter().map(|s| (s.ts, s.power_kw)).collect()`. Update any callers within the file.

- [x] T004 [US1] In `VEN/src/controller/reporter.rs` — remove the `soc_from_point(p: &HistoryPoint) -> Option<f64>` private helper entirely. Its one upstream use (EV SoC lookup in `build_measurement_report`) will be addressed in T007.

- [x] T005 [US1] In `VEN/src/controller/reporter.rs` — update `build_net_site_power_ts` to accept `samples: &HashMap<String, Vec<AssetReportSample>>` instead of `sim: &SimState`. Replace the body: iterate `samples.iter()`, for each `(_, asset_samples)` call `samples_to_power_ts(asset_samples, Interpolation::Step)`, keep all existing LOCF merge logic unchanged. Remove `let full_window = Duration::hours(2)` and `e.history.slice(...)` lines.

- [x] T006 [US1] In `VEN/src/controller/reporter.rs` — update `build_soc_intervals` to accept `samples: &HashMap<String, Vec<AssetReportSample>>` instead of `sim: &SimState`. Replace the EV/battery history extraction: `let soc_ts = samples.get("ev").filter(|v| !v.is_empty()).map(|v| { let ts_samples: Vec<(DateTime<Utc>, f64)> = v.iter().filter_map(|s| s.soc.map(|soc| (s.ts, soc))).collect(); TimeSeries { samples: ts_samples, interpolation: Interpolation::Step } }).filter(|ts| !ts.samples.is_empty()).or_else(|| /* same pattern for "battery" */...)`. Keep all interval-grid and resampling logic unchanged.

- [x] T007 [US1] In `VEN/src/controller/reporter.rs` — update `build_measurement_report_for_obligation` signature to `(obligation: &OadrReportObligation, asset_samples: &HashMap<String, Vec<AssetReportSample>>, ven_name: &str, site_envelope: Option<&SiteFlexibilityEnvelope>)`. Replace the two calls `build_net_site_power_ts(sim)` → `build_net_site_power_ts(asset_samples)` and `build_soc_intervals(sim, ...)` → `build_soc_intervals(asset_samples, ...)`. No other logic changes.

- [x] T008 [US1] In `VEN/src/controller/reporter.rs` — update `build_measurement_report` signature to `(event: &OadrEvent, asset_samples: &HashMap<String, Vec<AssetReportSample>>, grid_net_import_kw: f64, grid_net_export_kw: f64, ven_name: &str)`. Replace `latest_net_import_kw(sim)` with `grid_net_import_kw` directly. Replace `latest_net_export_kw(sim)` with `grid_net_export_kw` directly — note `grid_net_export_kw` is passed as a parameter but only consumed in the `EXPORT_CAPACITY_LIMIT` match arm (identical to the pre-change behavior of `latest_net_export_kw(sim)` in that arm; the parameter is unused in all other arms). Replace the EV SoC block (`sim.asset("ev")...soc_from_point`) with: `if let Some(soc) = asset_samples.get("ev").and_then(|v| v.last()).and_then(|s| s.soc)`. No other logic changes.

- [x] T009 [US1] In `VEN/src/controller/reporter.rs` — update `build_measurement_reports_for_active_events` signature to `(events: &[OadrEvent], asset_samples: &HashMap<String, Vec<AssetReportSample>>, grid_net_import_kw: f64, grid_net_export_kw: f64, ven_name: &str, now: DateTime<Utc>)`. Update the inner call `build_measurement_report(event, sim, ven_name)` → `build_measurement_report(event, asset_samples, grid_net_import_kw, grid_net_export_kw, ven_name)`.

- [x] T010 [US1] In `VEN/src/controller/reporter.rs` — update `latest_net_import_kw` to accept `snap: &SimSnapshot` instead of `sim: &SimState`. New body: `snap.assets.values().map(|a| a.power_kw).filter(|&kw| kw > 0.0).sum()`. Remove `latest_net_export_kw` entirely (its value is now passed as `grid_net_export_kw: f64` scalar from the callsite).

- [x] T011 [US1] In `VEN/src/controller/reporter.rs` — update `build_status_report` signature to `(event: &ControllerEvent, snap: &SimSnapshot, ven_name: &str, program_id: Option<&str>, _now: DateTime<Utc>)`. Two implementation options depending on T010 outcome: (a) if `latest_net_import_kw` was retained as a private helper accepting `&SimSnapshot`, replace `latest_net_import_kw(sim)` → `latest_net_import_kw(snap)`; (b) if it was inlined, replace directly with `snap.assets.values().map(|a| a.power_kw).filter(|&kw| kw > 0.0).sum::<f64>()`. Either option is correct — choose whichever results in cleaner code.

- [x] T012 [US1] In `VEN/src/controller/reporter.rs` — remove `use crate::assets::HistoryPoint;` and `use crate::simulator::SimState;` from the import block. Run `wsl cargo check -p ven --lib 2>&1 | grep "controller/reporter"` and confirm zero errors for this file (callers in other files will still fail at this point — that is expected).

- [x] T013 [US1] In `VEN/src/controller/reporter.rs` — rewrite the entire `#[cfg(test)] mod tests { ... }` block. Remove `make_sim`, `make_entry`, `make_ev_entry` helpers and all infra imports (`AssetHistoryBuffer`, `AssetState`, `BaseLoadState`, `EvState`, `Grid`, `HistoryPoint`, `SimState`, `EnergyCounter`, `GridMeter`). Add two new helpers:
  - `fn make_samples(id: &str, rows: &[(i64, f64)]) -> (String, Vec<AssetReportSample>)` — builds a Vec of AssetReportSample from `(offset_secs, power_kw)` pairs, pinning timestamps to just before `Utc::now()` (same logic as old `make_entry`).
  - `fn make_ev_samples(id: &str, rows: &[(i64, f64, f64)]) -> (String, Vec<AssetReportSample>)` — same but with SoC column.
  - `fn make_snap(assets: &[(&str, f64)]) -> SimSnapshot` — builds a `SimSnapshot` with given `(id, power_kw)` pairs, `asset_type: "base_load"`, all other fields zero.
  Update all existing test bodies to use these helpers. Delete the two `soc_from_point_*` tests. Rename `points_to_power_ts_basic` to `samples_to_power_ts_basic` using `AssetReportSample` inputs. Update `latest_net_import_kw_*` tests to pass `make_snap(...)` instead of the old `make_sim(...)` (function now accepts `&SimSnapshot`). Delete `latest_net_export_kw_sums_negative_assets_as_positive` test entirely (`latest_net_export_kw` is removed in T010). Preserve all other numeric assertions unchanged (same expected values, same tolerances).

- [x] T014 [US1] In `VEN/src/controller/reporter.rs` — add SC-004 unit test `build_measurement_report_domain_only` inside `#[cfg(test)] mod tests`. Construct a `HashMap<String, Vec<AssetReportSample>>` with one key `"site"` and two `AssetReportSample` entries (latest `power_kw: 3.0`). Call `build_measurement_report(&event, &samples, 3.0, 0.0, "ven-1")`. Assert the result `is_some()`, `programID == "prog-001"`, `clientName == "ven-1"`, USAGE payload value is `~3000.0_f64` (within 1.0 W). Confirm no `SimState`, `AssetHistoryBuffer`, or `HistoryPoint` is referenced.

- [x] T015 [US1] In `VEN/src/controller/reporter.rs` — add SC-005 unit test `build_status_report_domain_only` inside `#[cfg(test)] mod tests`. Construct `SimSnapshot` using `make_snap(&[("site", 2.0)])`. Call `build_status_report(&ctrl_event, &snap, "ven-1", Some("prog-001"), Utc::now())`. Assert result `is_some()`, `programID == "prog-001"`, `clientName == "ven-1"`, USAGE payload value is `~2000.0_f64` (within 1.0 W), TELEMETRY_STATUS payload contains "PlanCycle". Confirm no `SimState` is referenced.

- [x] T016 [US1] Run `wsl cargo test -p ven controller::reporter` — fix any compilation errors or test failures in `VEN/src/controller/reporter.rs` before proceeding to Phase 4.

**Checkpoint**: `reporter.rs` compiles with no infra imports, all existing tests pass plus SC-004/SC-005. ✓

---

## Phase 4: User Story 2 — Callers Extract History at the Boundary (Priority: P1)

**Goal**: All three external callers of public reporter functions pass domain types. Simulator lock is never held across a reporter call.

**Independent Test**: `wsl cargo check -p ven` exits 0 with no errors. Lock discipline verified by inspection (SC-006).

### Implementation for User Story 2

- [x] T017 [P] [US2] In `VEN/src/services/obligation.rs::check_and_report` — replace the existing lock block: acquire lock → extract `HashMap<String, Vec<AssetReportSample>>` by iterating `sim_guard.assets.iter()` and calling `entry.history.slice(chrono::Duration::hours(2), now)` per asset (use `Duration::hours(2)` — this matches the reporter's pre-change internal `full_window` constant), mapping each `HistoryPoint` to `AssetReportSample { ts: p.ts, power_kw: p.power_kw, soc: p.state.soc() }` → collect into `HashMap<String, Vec<AssetReportSample>>` keyed by `entry.id.clone()` → drop the lock. **Important**: the extraction loop must contain no `.await` points — `AssetHistoryBuffer::slice` is synchronous. Then call `build_measurement_report_for_obligation(&ob, &asset_samples, ven_name, env.as_ref())`. Add necessary imports: `use crate::controller::reporter::AssetReportSample;`.

- [x] T018 [P] [US2] In `VEN/src/tasks/planning.rs` — inside the status-report block (around line 233), remove the line `let sim_snap = sim.lock().await.clone()` (which produced a `SimState` clone). The outer `sim_snap: SimSnapshot` computed at line 219 (`sim.lock().await.to_sim_snapshot()`) is already in scope. Pass `&sim_snap` (now `&SimSnapshot`) directly to `build_status_report`. No other changes needed.

- [x] T019 [P] [US2] In `VEN/src/tasks/sim_tick/publish.rs::run_measurement_reports` — add parameter `sim_snap: SimSnapshot` (owned, not a reference — T020 passes `tick_sim_snap.clone()`) to the function signature. Inside the existing lock block, after acquiring `sim_guard`, extract `HashMap<String, Vec<AssetReportSample>>` by iterating `sim_guard.assets.iter()` with `Duration::hours(2)` window (same canonical constant as T017 — `Duration::hours(2)` is the pre-change reporter internal `full_window`), mapping `HistoryPoint → AssetReportSample { ts: p.ts, power_kw: p.power_kw, soc: p.state.soc() }`. **Important**: the extraction loop must contain no `.await` points — `AssetHistoryBuffer::slice` is synchronous. Drop the lock. Compute `grid_net_import_kw: f64 = sim_snap.assets.values().map(|a| a.power_kw).filter(|&kw| kw > 0.0).sum()` and `grid_net_export_kw: f64 = sim_snap.assets.values().map(|a| a.power_kw).filter(|&kw| kw < 0.0).map(|kw| -kw).sum()`. Call `build_measurement_reports_for_active_events(&events, &asset_samples, grid_net_import_kw, grid_net_export_kw, ven_name, now)`. Add `use crate::controller::reporter::AssetReportSample; use crate::controller::SimSnapshot;`.

- [x] T020 [US2] In `VEN/src/tasks/sim_tick/tick.rs` — update the call to `super::publish::run_measurement_reports` at line 181 to pass `tick_sim_snap.clone()` as the new `sim_snap` argument. (`tick_sim_snap: SimSnapshot` is already in scope from the `finalize_tick_outputs` call at line 127.)

- [x] T021 [US2] Run `wsl cargo check -p ven` — fix all remaining compilation errors (expected to be only in the caller files from Phase 4). Verify lock discipline structurally: run `grep -n "sim_guard\|build_measurement_reports_for_active\|build_measurement_report_for_ob" VEN/src/tasks/sim_tick/publish.rs VEN/src/services/obligation.rs` and confirm in the output that in both files the `sim_guard` line number (lock acquisition) appears **before** the drop/scope-end line, and the reporter call line appears **after** the drop/scope-end. This structural grep satisfies SC-006 in a machine-verifiable way.

**Checkpoint**: `wsl cargo check -p ven` exits 0. All callers pass domain types. ✓

---

## Phase 5: User Story 3 — Architecture Invariants Verified (Priority: P2)

**Goal**: Machine-verifiable proof that the fix is complete and cannot regress. BDD suite confirms no payload regressions.

**Independent Test**: All invariant greps return empty. `wsl cargo test -p ven` passes. BDD suite passes unchanged.

- [x] T022 [P] [US3] Run `grep "use crate::simulator\|use crate::assets" VEN/src/controller/reporter.rs` — must return empty (exit 1). This satisfies SC-001.

- [x] T023 [P] [US3] Run `grep -r "use crate::simulator\|use crate::assets" VEN/src/controller` — must return empty across the entire controller directory.

- [x] T024 [US3] Run `wsl cargo test -p ven` — all tests must pass. Confirm SC-004 (`build_measurement_report_domain_only`) and SC-005 (`build_status_report_domain_only`) appear in the output (SC-003). Fix any failures.

- [x] T025 [US3] Run the BDD integration suite on Pi4-Server to satisfy SC-007: `ssh Pi4-Server "cd /srv/docker/openadr_lab && git pull && docker compose -f tests/docker-compose.test.yml run --build --rm test-runner"`. Confirm all existing reporter-related scenarios pass (payload field names, value types, and serialisation shapes unchanged). Fix any regressions before proceeding — per Constitution Principle II, all acceptance scenarios must have BDD coverage and zero failures.

**Checkpoint**: All success criteria satisfied: SC-001 (invariant grep), SC-002 (cargo check), SC-003 (cargo test), SC-004/SC-005 (new unit tests), SC-006 (lock discipline), SC-007 (BDD suite). ✓

---

## Phase 6: Polish

- [x] T026 Update `docs/history/project_journal.md` — record: (1) what changed (`AssetReportSample` replaces `&SimState` in reporter.rs, three callers updated), (2) why (Hexagonal Architecture violation AB-05/AB-06 closed), (3) key learnings (history extraction window must match pre-change value of 2h; `planning.rs` re-lock was unnecessary since SimSnapshot was already in scope; `grid_net_export_kw` is only consumed in the `EXPORT_CAPACITY_LIMIT` branch — unused in other arms is acceptable).

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Setup)**: No dependencies — start immediately
- **Phase 2 (Foundational)**: Depends on Phase 1 — BLOCKS Phase 3 and Phase 4
- **Phase 3 (US1)**: Depends on Phase 2 — must complete before Phase 4
- **Phase 4 (US2)**: Depends on Phase 3 (reporter.rs must compile with new API before callers can compile)
- **Phase 5 (US3)**: Depends on Phase 4 — verification only
- **Phase 6 (Polish)**: Depends on Phase 5

### User Story Dependencies

- **US1**: Blocked only by Phase 2 (AssetReportSample definition)
- **US2**: Blocked by US1 (must compile reporter.rs with new signatures before fixing callers)
- **US3**: Depends on US2 completion (invariant checks run after all code is clean)

### Within Phase 3 (US1) — reporter.rs changes are sequential (same file)

The ordering within Phase 3 must be respected:
1. T003 (rename `points_to_power_ts`) — used by T005
2. T004 (remove `soc_from_point`) — used by T006/T008 logic
3. T005 (update `build_net_site_power_ts`) — used by T007
4. T006 (update `build_soc_intervals`) — used by T007
5. T007 (update `build_measurement_report_for_obligation`) — independent of T008/T009
6. T008 (update `build_measurement_report`) — uses T003/T004 patterns
7. T009 (update `build_measurement_reports_for_active_events`) — depends on T008
8. T010 (update `latest_net_import_kw` + remove `latest_net_export_kw`) — used by T011
9. T011 (update `build_status_report`) — depends on T010
10. T012 (remove infra imports) — must be LAST among implementation tasks
11. T013 (rewrite test module) — after all production code is correct
12. T014–T015 (new SC-004/SC-005 tests)
13. T016 (verify cargo test passes)

### Parallel Opportunities

**Phase 4 (US2)**: T017, T018, T019 are in different files and can be done in parallel:
- T017: `services/obligation.rs`
- T018: `tasks/planning.rs`
- T019: `tasks/sim_tick/publish.rs`

T020 (`tick.rs`) depends on T019's new signature — must follow T019.

**Phase 5 (US3)**: T022 and T023 (invariant greps) can run in parallel. T024 and T025 are sequential (T025 BDD run should follow T024 cargo test pass).

---

## Parallel Example: Phase 4 (US2)

```
# These three files can be updated concurrently:
Task T017: Update services/obligation.rs
Task T018: Update tasks/planning.rs
Task T019: Update tasks/sim_tick/publish.rs

# Only after T019:
Task T020: Update tasks/sim_tick/tick.rs (depends on T019 new signature)
```

---

## Implementation Strategy

### MVP First (User Story 1 only)

1. Complete Phase 1 (baseline)
2. Complete Phase 2 (add AssetReportSample)
3. Complete Phase 3 (US1: all reporter.rs changes + new tests)
4. **STOP and VALIDATE**: `wsl cargo test -p ven controller::reporter` passes
   - Note: full build will fail (callers not updated yet) — that is expected at this point

### Full Delivery (all user stories)

5. Complete Phase 4 (US2: update callers)
6. **VALIDATE**: `wsl cargo check -p ven` exits 0
7. Complete Phase 5 (US3: invariant verification + full test run + BDD suite)
8. Complete Phase 6 (journal)

---

## Notes

- All changes are in `VEN/src/` — no `Cargo.toml` changes, no new files
- `AssetReportSample` is defined in `reporter.rs` and imported by the three caller files
- History window for `obligation.rs` and `publish.rs` extraction: `chrono::Duration::hours(2)` (matches pre-change `build_net_site_power_ts` internal constant)
- FR-008: **no logic changes permitted** — all numeric assertions in tests should pass with identical expected values
- The lock-release pattern is the key invariant: extract data under lock, drop lock, call reporter
