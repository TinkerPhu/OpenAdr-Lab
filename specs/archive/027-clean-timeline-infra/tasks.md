# Tasks: Clean Timeline Infra Imports

**Input**: Design documents from `/specs/027-clean-timeline-infra/`
**Prerequisites**: plan.md ✓, spec.md ✓, research.md ✓, data-model.md ✓, quickstart.md ✓

**Organization**: Tasks grouped by user story. US1 (production code) must compile before US2 (test fixtures) can be validated. Phases 1–2 are foundational; Phases 3–4 map to US1/US2; Phase 5 is verification.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependency on incomplete tasks in this phase)
- **[Story]**: Which user story this task belongs to (US1, US2)

---

## Phase 1: Setup — Baseline Verification

**Purpose**: Record the before-state before touching any code.

- [x] T001 Run `grep "use crate::assets" VEN/src/controller/timeline.rs` and confirm it currently returns 2 lines (line 13: `AssetConfig, AssetHistoryBuffer, AssetState`). Run `wsl cargo test -p ven` and record that all existing tests pass (establishes baseline — confirms no pre-existing failures to confuse regression detection).

---

## Phase 2: Foundational — Domain Types and HeaterPlanTrajectory

**Purpose**: Define the new domain types and move `HeaterPlanTrajectory` to the domain ring. No function body changes yet — only struct definitions. **Blocks US1 and US2.**

**⚠️ CRITICAL**: `wsl cargo check -p ven` will fail after T004–T006 until T011 (function bodies updated to use new fields). That is expected — treat this phase as one atomic rewrite unit.

- [x] T002 In `VEN/src/controller/timeline.rs` — Add `use crate::entities::asset::AssetType;` to the import block (alongside the existing `use crate::entities::plan::Plan;`). Then add `pub struct TimelinePoint { pub ts: DateTime<Utc>, pub power_kw: f64, pub state_values: std::collections::HashMap<String, f64> }` after the import block, before `TimelineAssetData`. The `HashMap` import is already available via the existing `use std::collections::{HashMap, HashSet};`.

- [x] T003 In `VEN/src/controller/timeline.rs` — After `TimelinePoint`, add the moved `HeaterPlanTrajectory` struct and its `next_slot` implementation (copied verbatim from `VEN/src/assets/heater.rs` lines 330–357, with `#[derive(Clone)]` added to the struct):
  ```rust
  #[derive(Clone)]
  pub struct HeaterPlanTrajectory {
      pub e_kwh:        f64,
      pub temp_min_c:   f64,
      pub thermal_mass: f64,
      pub q_dem_kw:     f64,
      pub e_max_kwh:    f64,
  }
  impl HeaterPlanTrajectory {
      pub fn next_slot(&mut self, p_heat_kw: f64, dt_h: f64) -> HashMap<String, f64> {
          let temp_c = self.temp_min_c + self.e_kwh / self.thermal_mass;
          self.e_kwh = (self.e_kwh + (p_heat_kw - self.q_dem_kw) * dt_h)
              .clamp(0.0, self.e_max_kwh);
          HashMap::from([("temp_c".into(), temp_c)])
      }
  }
  ```

- [x] T004 In `VEN/src/assets/heater.rs` — Remove the `pub struct HeaterPlanTrajectory { ... }` and `impl HeaterPlanTrajectory { pub fn new(...) {...} pub fn next_slot(...) {...} }` blocks (lines ~330–357). Add `use crate::controller::timeline::HeaterPlanTrajectory;` at the top of the file (inside the existing `use` block). The `pub fn plan_trajectory(cfg: &Self, live_state: &super::AssetState) -> Option<HeaterPlanTrajectory>` method signature is unchanged — it now references the domain-ring type. Run `wsl cargo check -p ven 2>&1 | head -30` to confirm the only errors are about `TimelineAssetData`/`TimelineSnapshot` field mismatches (not the HeaterPlanTrajectory move). Fix any errors from the move itself before proceeding.

- [x] T005 In `VEN/src/controller/timeline.rs` — Replace the existing `pub struct TimelineAssetData` definition:
  ```rust
  // OLD:
  pub struct TimelineAssetData {
      pub history: AssetHistoryBuffer,
      pub config: AssetConfig,
      pub current_state: AssetState,
  }
  // NEW:
  pub struct TimelineAssetData {
      pub asset_id:             String,
      pub asset_type:           AssetType,
      pub history:              Vec<TimelinePoint>,
      pub current_power_kw:     f64,
      pub current_state_values: HashMap<String, f64>,
      pub plan_trajectory:      Option<HeaterPlanTrajectory>,
  }
  ```

- [x] T006 In `VEN/src/controller/timeline.rs` — Replace the existing `pub struct TimelineSnapshot` definition and remove the `use crate::assets::{AssetConfig, AssetHistoryBuffer, AssetState};` import line:
  ```rust
  // OLD:
  pub struct TimelineSnapshot {
      pub assets: HashMap<String, TimelineAssetData>,
      pub grid_history: AssetHistoryBuffer,
  }
  // NEW:
  pub struct TimelineSnapshot {
      pub assets:          HashMap<String, TimelineAssetData>,
      pub grid_history:    Vec<TimelinePoint>,
      pub grid_current_kw: f64,
  }
  ```
  Also remove `use crate::assets::{AssetConfig, AssetHistoryBuffer, AssetState};` from the import block.

**Checkpoint**: All struct definitions changed. `wsl cargo check` will fail on function bodies — expected. ✓

---

## Phase 3: User Story 1 — Timeline Rendering Preserved (Priority: P1) 🎯 MVP

**Goal**: Update all production function bodies to use the new domain-only types. After this phase `wsl cargo check -p ven` exits 0 and the timeline API response is identical to before.

**Independent Test**: `wsl cargo check -p ven` exits 0. Existing timeline BDD scenarios on Pi4-Server pass.

### Implementation for User Story 1

- [x] T007 [US1] In `VEN/src/simulator/mod.rs` — Add to the import block: `use crate::controller::timeline::{HeaterPlanTrajectory, TimelineAssetData, TimelinePoint, TimelineSnapshot};` and `use crate::entities::asset::AssetType;`. Then rewrite the body of `pub fn to_timeline_snapshot(&self) -> TimelineSnapshot`. Add `let now = chrono::Utc::now();` at the top. For each asset entry (via `self.iter_assets()`), build a `TimelineAssetData` with:
  - `asset_id`: `entry.id.clone()`
  - `asset_type`: derived from the config variant via match: `AssetConfig::Battery(_) => AssetType::Battery`, `Ev(_) => AssetType::Ev`, `Heater(_) => AssetType::Heater`, `Pv(_) => AssetType::Pv`, `BaseLoad(_) => AssetType::GenericConsumer`
  - `history`: `entry.history.slice(chrono::Duration::seconds(3600), now).into_iter().map(|p| TimelinePoint { ts: p.ts, power_kw: p.power_kw, state_values: cfg.state_values(&p.state) }).collect()`
  - `current_power_kw`: `entry.history.recent_avg_power(chrono::Duration::seconds(60), now).unwrap_or_else(|| entry.history.latest().map(|p| p.power_kw).unwrap_or(0.0))`
  - `current_state_values`: `cfg.state_values(&entry.state)`
  - `plan_trajectory`: built from a `match (&entry.config, &entry.state)` for the `(AssetConfig::Heater(cfg), AssetState::Heater(s))` arm (inline the `HeaterPlanTrajectory::new()` logic from the deleted `new()` method): `let e_max_kwh = (cfg.temp_max_c - cfg.temp_min_c) * cfg.thermal_mass_kwh_per_c; let e_kwh = ((s.temperature_c - cfg.temp_min_c) * cfg.thermal_mass_kwh_per_c).clamp(0.0, e_max_kwh); Some(HeaterPlanTrajectory { e_kwh, temp_min_c: cfg.temp_min_c, thermal_mass: cfg.thermal_mass_kwh_per_c, q_dem_kw: cfg.forecast_demand_kw(cfg.ambient_temp_c), e_max_kwh })`. All other variants return `None`.

- [x] T008 [US1] In `VEN/src/simulator/mod.rs` — Within the same `to_timeline_snapshot()` rewrite, build the grid fields:
  - `grid_history`: `self.grid_asset.history.slice(chrono::Duration::seconds(3600), now).into_iter().map(|p| TimelinePoint { ts: p.ts, power_kw: p.power_kw, state_values: HashMap::new() }).collect()`
  - `grid_current_kw`: `self.grid_asset.history.latest().map(|p| p.power_kw).unwrap_or(0.0)`
  Return `TimelineSnapshot { assets, grid_history, grid_current_kw }`. Verify `wsl cargo check -p ven 2>&1 | grep "simulator/mod.rs"` shows no errors in this file.

- [x] T009 [US1] In `VEN/src/controller/timeline.rs` — Rewrite `pub fn build_now_point(asset_id: &str, now: DateTime<Utc>, snap: &TimelineSnapshot) -> AssetTimelinePoint`. For sim assets (`snap.assets.get(asset_id)`): if the asset is found, build `values` from `data.current_state_values.clone()`, set `values.insert("power_kw".into(), data.current_power_kw)`, return `AssetTimelinePoint { ts: now, values }`. If the asset entry exists but has no data (empty history is fine — `current_power_kw` and `current_state_values` are always set by `to_timeline_snapshot`), return the point anyway. For the grid asset (`asset_id == "grid"`): use `snap.grid_current_kw` as the power value, return `AssetTimelinePoint { ts: now, values: HashMap::from([("power_kw".into(), snap.grid_current_kw)]) }`. Fall-through (unknown asset): return `AssetTimelinePoint { ts: now, values: HashMap::new() }` as before.

- [x] T010 [US1] In `VEN/src/controller/timeline.rs` — Rewrite the history section of `pub fn build_asset_timeline(...)`. For sim assets (the `if let Some(data) = snap.assets.get(asset_id)` arm): replace `data.history.slice(back_window, now).into_iter().filter(...).map(|p| { let mut values = data.config.state_values(&p.state); ... })` with `data.history.iter().filter(|p| p.ts >= past_start).map(|p| { let mut values = p.state_values.clone(); values.insert("power_kw".into(), p.power_kw); AssetTimelinePoint { ts: p.ts, values } }).collect()`. For the grid arm (`else if is_grid`): replace `snap.grid_history.slice(back_window, now).into_iter().filter(...)` with `snap.grid_history.iter().filter(|p| p.ts >= past_start).map(|p| { let mut values = HashMap::new(); values.insert("power_kw".into(), p.power_kw); AssetTimelinePoint { ts: p.ts, values } }).collect()`.

- [x] T011 [US1] In `VEN/src/controller/timeline.rs` — Rewrite the plan_trajectory section of `build_asset_timeline`. Replace `let mut plan_traj = snap.assets.get(asset_id).and_then(|d| d.config.plan_trajectory(&d.current_state));` with `let mut plan_traj = snap.assets.get(asset_id).and_then(|d| d.plan_trajectory.clone());`. The `plan_traj.as_mut().map(|traj| traj.next_slot(...))` usage inside the future-slot loop is unchanged.

- [x] T012 [US1] Run `wsl cargo check -p ven 2>&1` — fix all remaining compilation errors. Common issues to expect: unused imports left over in `controller/timeline.rs` (e.g., `AssetHistoryBuffer` still referenced somewhere), type mismatches in the `build_asset_timeline` future-slot loop (now reads `d.current_state_values` instead of `d.config.plan_trajectory`), or missing `HashMap` import in `simulator/mod.rs`. Resolve all errors until `cargo check` exits 0.

**Checkpoint**: `wsl cargo check -p ven` exits 0. Timeline rendering unchanged. ✓

---

## Phase 4: User Story 2 — Infra-Free Unit Tests (Priority: P2)

**Goal**: Replace all infra-type fixture helpers in `controller/timeline.rs` test module with domain-only equivalents. After this phase `grep "use crate::assets" VEN/src/controller/timeline.rs` returns zero matches.

**Independent Test**: `wsl cargo test -p ven controller::timeline` passes with all existing test cases green.

### Implementation for User Story 2

- [x] T013 [US2] In `VEN/src/controller/timeline.rs` (test module `#[cfg(test)]`) — Rewrite `fn make_base_snap(id: &str, rows: &[(i64, f64)]) -> (String, TimelineAssetData)`. Remove all construction of `AssetConfig::BaseLoad(...)`, `AssetHistoryBuffer::new(...)`, `AssetState::BaseLoad(...)`, `HistoryPoint { ... }`. Replace body:
  ```rust
  fn make_base_snap(id: &str, rows: &[(i64, f64)]) -> (String, TimelineAssetData) {
      let history: Vec<TimelinePoint> = rows
          .iter()
          .map(|(offset, power)| TimelinePoint {
              ts: ts(*offset),
              power_kw: *power,
              state_values: std::collections::HashMap::from([
                  ("baseline_kw".to_string(), 0.0_f64),
              ]),
          })
          .collect();
      let current_power_kw = history.last().map(|p| p.power_kw).unwrap_or(0.0);
      let current_state_values =
          std::collections::HashMap::from([("baseline_kw".to_string(), 0.0_f64)]);
      (
          id.to_string(),
          TimelineAssetData {
              asset_id: id.to_string(),
              asset_type: AssetType::GenericConsumer,
              history,
              current_power_kw,
              current_state_values,
              plan_trajectory: None,
          },
      )
  }
  ```

- [x] T014 [US2] In `VEN/src/controller/timeline.rs` (test module) — Rewrite `fn make_ev_snap(id: &str, rows: &[(i64, f64, f64)]) -> (String, TimelineAssetData)`. Remove all construction of `AssetConfig::Ev(...)`, `AssetHistoryBuffer`, `AssetState::Ev(...)`, `HistoryPoint { ... }`. Replace body:
  ```rust
  fn make_ev_snap(id: &str, rows: &[(i64, f64, f64)]) -> (String, TimelineAssetData) {
      let history: Vec<TimelinePoint> = rows
          .iter()
          .map(|(offset, power, soc)| TimelinePoint {
              ts: ts(*offset),
              power_kw: *power,
              state_values: std::collections::HashMap::from([
                  ("soc".to_string(), *soc),
                  ("plugged".to_string(), 1.0_f64),
              ]),
          })
          .collect();
      let last = history.last();
      let current_power_kw = last.map(|p| p.power_kw).unwrap_or(0.0);
      let current_state_values = last
          .map(|p| p.state_values.clone())
          .unwrap_or_default();
      (
          id.to_string(),
          TimelineAssetData {
              asset_id: id.to_string(),
              asset_type: AssetType::Ev,
              history,
              current_power_kw,
              current_state_values,
              plan_trajectory: None,
          },
      )
  }
  ```

- [x] T015 [US2] In `VEN/src/controller/timeline.rs` (test module) — Rewrite `fn make_timeline_snap(entries: Vec<(String, TimelineAssetData)>) -> TimelineSnapshot`. Replace body:
  ```rust
  fn make_timeline_snap(entries: Vec<(String, TimelineAssetData)>) -> TimelineSnapshot {
      TimelineSnapshot {
          assets: entries.into_iter().collect(),
          grid_history: vec![],
          grid_current_kw: 0.0,
      }
  }
  ```

- [x] T016 [US2] In `VEN/src/controller/timeline.rs` (test module) — Update the three `build_now_point` tests that construct `TimelineAssetData` directly with infra types:
  - **`build_now_point_uses_recent_average_for_power`**: Rewrite to use `make_ev_snap("ev", &[(99, 2.0, 0.6)])` (which sets `current_power_kw = 2.0` and `current_state_values = {"soc": 0.6, "plugged": 1.0}`). Assertions unchanged (`power_kw == 2.0`, `soc == 0.6`).
  - **`build_now_point_smooths_oscillating_power`**: This test previously verified that the 60s rolling average is applied. After the refactoring, `build_now_point` reads `current_power_kw` directly — the smoothing computation moved to `to_timeline_snapshot()`. Rewrite the test to verify that whatever `current_power_kw` is set in the fixture is returned unchanged: construct a `TimelineAssetData` with `current_power_kw: 1.25` and `history: vec![]` (the history is irrelevant — the pre-computed value is what matters). Assert `np.values["power_kw"] == 1.25`. Add a one-line comment: `// smoothing computed in to_timeline_snapshot(); this test verifies build_now_point reads the pre-computed value`.
  - **`build_now_point_empty_history`**: Already uses `make_timeline_snap(vec![])` — no direct infra construction. Verify it still compiles and passes.

- [x] T017 [US2] In `VEN/src/controller/timeline.rs` (test module) — Remove all infra imports from the `use` block inside `#[cfg(test)] mod tests`. Remove: `use crate::assets::{AssetConfig, AssetHistoryBuffer, AssetState, BaseLoad, BaseLoadState, EvCharger, EvState, HistoryPoint};`. Add `use crate::entities::asset::AssetType;` if not already present. Confirm the test module now has zero `crate::assets` references (do a quick scan of the test module body).

- [x] T018 [US2] Run `wsl cargo test -p ven controller::timeline 2>&1` — all existing tests must pass. If any test fails due to a fixture change (e.g., an assertion value now differs because `make_base_snap` or `make_ev_snap` supplies different state_values), investigate and fix the fixture — do not weaken the assertion. The numeric invariants tested by `past_only_window_returns_history_points`, `future_only_window_returns_plan_points`, etc. must remain identical.

**Checkpoint**: All timeline unit tests pass. Zero `use crate::assets` in `controller/timeline.rs`. ✓

---

## Phase 5: Verification & Polish

**Purpose**: Confirm all success criteria, commit, and run BDD on Pi4-Server.

- [x] T019 [P] Run invariant grep SC-001: `grep "use crate::assets" VEN/src/controller/timeline.rs` — must return zero matches.

- [x] T020 [P] Run invariant grep SC-002: `grep "use crate::simulator" VEN/src/controller/timeline.rs` — must return zero matches.

- [x] T021 Run full unit test suite SC-003: `wsl cargo test -p ven 2>&1` — all tests must pass. Record the passing count.

- [x] T022 Check file size constraint: `wsl wc -l VEN/src/controller/timeline.rs VEN/src/simulator/mod.rs` — `simulator/mod.rs` must be ≤ 500 lines. Note `controller/timeline.rs` line count (pre-existing violation; document but do not fix here).

- [x] T023 Commit: `git add VEN/src/controller/timeline.rs VEN/src/assets/heater.rs VEN/src/simulator/mod.rs` then commit with message `feat(027): remove infra imports from controller/timeline.rs (VG-03)`.

- [x] T024 Push branch and deploy to Pi4-Server: `git push origin 027-clean-timeline-infra` then `ssh Pi4-Server "cd /srv/docker/openadr_lab && git pull"`.

- [x] T025 Run BDD suite on Pi4-Server: `ssh Pi4-Server "cd /srv/docker/openadr_lab && docker compose -f tests/docker-compose.test.yml run --build --rm test-runner"`. Confirm all scenarios pass (SC-004). If any failures occur, check `/tmp/` on Pi4-Server for the full log and investigate before proceeding — per the constitution, all failures must be resolved, not classified away.

- [x] T026 Update `docs/history/project_journal.md` — record: (1) what changed (VG-03 closed: `AssetConfig`, `AssetHistoryBuffer`, `AssetState` removed from `controller/timeline.rs`; `HeaterPlanTrajectory` moved to domain ring; `to_timeline_snapshot()` now pre-computes `state_values`, `current_power_kw`, and trajectory seed at the infra boundary), (2) why (Hexagonal Architecture dependency rule — domain ring must never import infra ring; VG-03 was the last domain-ring violation in `controller/`), (3) key learnings from implementation.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Setup)**: No dependencies — start immediately
- **Phase 2 (Foundational)**: Depends on Phase 1 — **BLOCKS** Phase 3 and 4
- **Phase 3 (US1)**: Depends on Phase 2 — production code updates
- **Phase 4 (US2)**: Depends on Phase 3 (`cargo check` must exit 0 before test fixtures make sense to update)
- **Phase 5 (Verification)**: Depends on Phase 4 — all implementation and tests complete

### Within Phase 2 (Foundational)

Sequential within `controller/timeline.rs` (same file):
1. T002 (add imports + TimelinePoint)
2. T003 (add HeaterPlanTrajectory to timeline.rs)
3. T004 (move HeaterPlanTrajectory out of heater.rs) — T002 must precede T003 must precede T004
4. T005 (update TimelineAssetData)
5. T006 (update TimelineSnapshot + remove infra import)

### Within Phase 3 (US1)

- T007 and T008 are in `simulator/mod.rs` — do together as one rewrite of `to_timeline_snapshot()`
- T009, T010, T011 are in `controller/timeline.rs` — sequential (same file)
- T012 (cargo check + fix) — must follow T007–T011

### Within Phase 4 (US2)

- T013, T014, T015 are in `#[cfg(test)]` (same file) — sequential
- T016 follows T013–T015 (test bodies reference updated fixtures)
- T017 follows T016 (import cleanup after all test bodies updated)
- T018 follows T017 (run tests only after fixtures + imports are clean)

### Parallel Opportunities

- T019 and T020 (invariant greps) can run in parallel
- T007 and T008 are effectively one task (rewrite of `to_timeline_snapshot()` in one sitting)
- T013, T014, T015 can be done in any order (different functions, same file section)

---

## Parallel Example: Phase 5 Verification

```
# Launch both invariant greps together:
Task T019: grep "use crate::assets" VEN/src/controller/timeline.rs
Task T020: grep "use crate::simulator" VEN/src/controller/timeline.rs
```

---

## Implementation Strategy

### MVP First (User Story 1 only)

1. Complete Phase 1: Baseline
2. Complete Phase 2: Foundational types
3. Complete Phase 3: US1 production code → `cargo check` exits 0
4. **STOP and VALIDATE**: Timeline API behavior verified (cargo check, manual smoke test)

### Full Delivery

5. Complete Phase 4: US2 infra-free tests
6. **VALIDATE**: `cargo test controller::timeline` all green
7. Complete Phase 5: Invariant greps, full cargo test, BDD, commit, journal

---

## Notes

- All changes are in `VEN/src/` — no `Cargo.toml` changes, no new files
- The `HeaterPlanTrajectory` struct move (T003 + T004) must be atomic: define in domain first (T003), remove from infra (T004), so the compiler never sees two definitions
- `to_timeline_snapshot()` introduces `let now = chrono::Utc::now()` locally — this is consistent with how other methods in the same file acquire current time
- The `build_now_point_smooths_oscillating_power` test semantics change: the assertion "smoothing is applied" moves from domain (build_now_point) to infra (to_timeline_snapshot). The domain test now verifies "pre-computed value is passed through unchanged" — which is the correct invariant for the domain layer
- `controller/timeline.rs` at 1316 lines is a pre-existing 500-line violation (documented in plan.md Complexity Tracking). This feature will shrink the file slightly but not resolve it. File splitting is a follow-up task
- After T006, `AssetHistoryBuffer` has no remaining uses in `controller/timeline.rs` — verify this before finalizing T006
