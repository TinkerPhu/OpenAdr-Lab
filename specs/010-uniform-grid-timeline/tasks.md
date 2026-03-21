# Tasks: Uniform-Grid Timeline API

**Input**: Design documents from `/specs/010-uniform-grid-timeline/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/

**Tests**: BDD tests included per constitution (BDD-First Testing principle).

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

---

## Phase 1: Setup

**Purpose**: No new project setup needed — this feature modifies existing files. Phase is empty.

---

## Phase 2: Foundational (Core Resampling Logic)

**Purpose**: Pure resampling functions that all user stories depend on. Must complete before any handler changes.

- [x] T001 Implement `compute_uniform_grid()` in `VEN/src/controller/timeline.rs` — takes `window_start`, `window_end`, `now`, `resolution_s` and returns `(history_timestamps: Vec<DateTime>, future_timestamps: Vec<DateTime>)` with timestamps snapped to round boundaries of resolution. Include unit tests: verify uniform spacing, round-boundary snapping, determinism (same inputs = same grid), edge case where `now` falls exactly on a grid boundary.

- [x] T002 Implement `resample_to_grid()` in `VEN/src/controller/timeline.rs` — takes a `Vec<AssetTimelinePoint>` (raw sorted points) and a `Vec<DateTime>` (grid timestamps) and returns `Vec<AssetTimelinePoint>` with one entry per grid timestamp. History buckets use LOCF time-weighted mean; empty buckets get `values: None`. Include unit tests: verify LOCF aggregation with multiple rows per bucket, single row per bucket, empty buckets produce null, NaN-only buckets produce null.

- [x] T003 Implement `build_now_point()` in `VEN/src/controller/timeline.rs` — takes `asset_id`, `now`, `history` (AssetHistoryBuffer), and `known_assets` and returns an `AssetTimelinePoint` at exact `now` with instantaneous values from the most recent history row. For the "grid" virtual asset, compute from the latest history row if available. Include unit tests: verify `ts == now`, values match last history row, empty history returns NaN-only point.

- [x] T004 Update `serialize_timeline()` in `VEN/src/main.rs` to emit `"values": null` (JSON null) when an `AssetTimelinePoint` has `values: None` or all-NaN values, instead of omitting the entry. The `ts` field must always be present.

**Checkpoint**: Core resampling logic complete and unit-tested. Handlers not yet changed.

---

## Phase 3: User Story 1 + 2 — Grid-Aligned Timeline with Now-Point (Priority: P1) MVP

**Goal**: `GET /timeline/all` returns all assets on a shared uniform grid with a now-point, in the unchanged response format.

**Independent Test**: Call `GET /timeline/all`, verify all assets have identical array lengths and identical `ts` at each index. Verify the now-point sits between history and future grid portions.

### BDD Tests for US1+US2

- [x] T005 [US1] Write BDD feature file `tests/features/timeline_grid.feature` with scenarios:
  1. GET /timeline/all returns arrays of equal length for all assets
  2. All assets share the same ts value at each index position
  3. Grid-portion timestamps are uniformly spaced
  4. Grid timestamps are snapped to round boundaries (e.g., resolution=10 gives multiples of 10)
  5. Each asset array contains a now-point between history and future grid portions
  6. The now-point ts is the same across all assets
  7. Empty future buckets have values null
  8. Response format is unchanged (object with asset keys, each value is an array of {ts, values})

- [x] T006 [US1] Write step definitions in `tests/features/steps/timeline_grid_steps.py` — implement all steps for the scenarios in T005. Use existing VEN HTTP helpers.

### Implementation for US1+US2

- [x] T007 [US1] Add `resolution` field to `TimelineParams` struct in `VEN/src/main.rs`. Add `resolve_resolution()` helper that returns `resolution` if set, else converts `max_points` via `ceil(total_window_s / max_points)`, else auto-calculates `ceil(total_window_s / 300)`. Clamp grid points to max 3600.

- [x] T008 [US1] Rewrite `get_timeline_all()` handler in `VEN/src/main.rs`: compute the shared uniform grid once via `compute_uniform_grid()`, then for each asset: (1) call `build_asset_timeline()` for raw points, (2) split raw points into history (ts < now) and future (ts >= now), (3) resample history onto `history_timestamps` via `resample_to_grid()`, (4) resample future onto `future_timestamps` via `resample_to_grid()`, (5) build now-point via `build_now_point()`, (6) concatenate `[...history_grid, now_point, ...future_grid]`. Remove `downsample()` calls.

- [x] T009 [US1] Update existing BDD scenarios in `tests/features/ven_timeline.feature` that may break: "every timeline point has a values object" must now tolerate `null` values for empty grid buckets. Update step definitions in `tests/features/steps/ven_timeline_steps.py` if needed.

**Checkpoint**: `GET /timeline/all` returns grid-aligned arrays with now-point. All assets have identical lengths and ts values. BDD tests pass.

---

## Phase 4: User Story 3 — Resolution Parameter (Priority: P2)

**Goal**: `resolution` query parameter controls grid bucket width; `max_points` works as deprecated alias.

**Independent Test**: Call with `resolution=30`, verify 30-second spacing. Call with `max_points=150`, verify equivalent behavior. Call with both, verify `resolution` wins.

### BDD Tests for US3

- [x] T010 [US3] Add BDD scenarios to `tests/features/timeline_grid.feature`:
  1. GET /timeline/all?resolution=30 returns 30-second spacing in grid portions
  2. GET /timeline/all with no resolution auto-calculates targeting ~300 points
  3. GET /timeline/all?max_points=150 produces equivalent resolution
  4. GET /timeline/all?resolution=30&max_points=150 uses resolution=30 (resolution wins)

- [x] T011 [US3] Implement step definitions for T010 scenarios in `tests/features/steps/timeline_grid_steps.py`.

### Implementation for US3

- [x] T012 [US3] Verify `resolve_resolution()` from T007 handles all cases: resolution-only, max_points-only, both (resolution wins), neither (auto ~300). Add cargo unit tests for each case in `VEN/src/main.rs` or a test module.

**Checkpoint**: Resolution parameter works. Deprecated max_points still functions. BDD tests pass.

---

## Phase 5: User Story 4 — Single-Asset Endpoint (Priority: P3)

**Goal**: `GET /timeline/:asset_id` applies the same uniform grid resampling with now-point.

**Independent Test**: Call `GET /timeline/ev`, verify uniform spacing with now-point. Call `GET /timeline/xyz`, verify 404.

### BDD Tests for US4

- [x] T013 [US4] Add BDD scenarios to `tests/features/timeline_grid.feature`:
  1. GET /timeline/ev returns uniformly spaced ts with now-point
  2. GET /timeline/ev?resolution=30 returns 30-second spacing
  3. GET /timeline/unknown_asset_xyz returns 404 (unchanged)

- [x] T014 [US4] Implement step definitions for T013 scenarios in `tests/features/steps/timeline_grid_steps.py`.

### Implementation for US4

- [x] T015 [US4] Rewrite `get_timeline()` handler in `VEN/src/main.rs` to use the same grid resampling + now-point logic as `get_timeline_all()`. Extract shared logic into a helper if duplication is significant (but prefer inline if it's just a few lines).

**Checkpoint**: Single-asset endpoint uses uniform grid. BDD tests pass. 404 behavior unchanged.

---

## Phase 6: Polish & Cross-Cutting Concerns

- [x] T016 Remove the old `downsample()` function from `VEN/src/main.rs` if no longer referenced.
- [x] T017 Run full BDD test suite (`tests/features/`) to verify no regressions across all existing scenarios.
- [ ] T018 Update `docs/history/project_journal.md` with implementation summary, decisions, and key learnings.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 2 (Foundational)**: No dependencies — can start immediately
- **Phase 3 (US1+US2)**: Depends on Phase 2 completion
- **Phase 4 (US3)**: Depends on Phase 3 (resolution logic used in the handler)
- **Phase 5 (US4)**: Depends on Phase 3 (reuses the same grid+now-point logic)
- **Phase 6 (Polish)**: Depends on all user stories being complete

### User Story Dependencies

- **US1+US2 (P1)**: Depends on foundational resampling functions (T001-T004)
- **US3 (P2)**: Depends on US1+US2 (resolution parameter plugs into the handler built in US1)
- **US4 (P3)**: Depends on US1+US2 (reuses the grid resampling logic). Can run in parallel with US3.

### Within Each User Story

- BDD feature file + steps written first (constitution: BDD-first)
- Handler implementation after BDD scenarios exist
- Existing test fixes as needed

### Parallel Opportunities

- T001, T002, T003 can run in parallel (independent functions in same file, but different functions)
- T005 and T006 can run in parallel with T007 (BDD tests vs. Rust code)
- T010/T011 (US3 BDD) can run in parallel with T013/T014 (US4 BDD) after US1+US2 is complete
- US3 (Phase 4) and US4 (Phase 5) can run in parallel after Phase 3

---

## Implementation Strategy

### MVP First (US1+US2 Only)

1. Complete Phase 2: Foundational resampling functions
2. Complete Phase 3: US1+US2 — grid-aligned /timeline/all with now-point
3. **STOP and VALIDATE**: Run BDD tests, verify grid alignment, check existing tests pass
4. This alone fixes the core chart misalignment problem

### Incremental Delivery

1. Phase 2 → Foundational functions ready
2. Phase 3 (US1+US2) → Grid-aligned /timeline/all with now-point (MVP)
3. Phase 4 (US3) → Resolution parameter + max_points compat
4. Phase 5 (US4) → Single-asset endpoint aligned
5. Phase 6 → Cleanup, full regression, journal

---

## Notes

- Response format is unchanged (`Record<string, {ts, values}[]>`) — no UI client/hook changes needed for format parsing
- The UI will need to handle `values: null` entries (RF-05d scope, not this task)
- `build_asset_timeline()` is kept as-is — it produces raw points that are then resampled onto the grid
- The now-point is NOT a grid point — it has one irregular gap in spacing, which is expected and documented
