# Tasks: Planner Slot Costing — Configurable Aggregation

**Input**: Design documents from `/specs/013-slot-costing-aggregation/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md

**Tests**: Unit tests are included — this is a pure infrastructure change verified by cargo test.

**Organization**: Tasks are grouped by user story. US3 (configurable aggregation) is the enabler for US1 and US2, so it appears first as foundational.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

---

## Phase 1: Foundational — Aggregation Infrastructure (US3, Priority: P2)

**Goal**: Add configurable aggregation mode to `TimeSeries::resample_uniform()` so callers can choose Mean, Min, or Max bucket reduction.

**Independent Test**: `cargo test common::tests` — all existing tests pass with `Aggregation::Mean`, new min/max tests verify correct behavior.

- [ ] T001 [US3] Add `Aggregation` enum (Mean, Min, Max) to `VEN/src/common/mod.rs`
- [ ] T002 [US3] Implement `bucket_extreme()` shared helper method in `VEN/src/common/mod.rs` — takes a `pick: impl Fn(f64, f64) -> f64` closure, evaluates signal at bucket start + all interior sample points, and for Linear also at bucket end
- [ ] T003 [P] [US3] Implement `bucket_min()` and `bucket_max()` as thin wrappers around `bucket_extreme()` in `VEN/src/common/mod.rs`
- [ ] T004 [US3] Update `resample_uniform()` signature to accept `Aggregation` parameter and dispatch to `time_weighted_mean`, `bucket_min`, or `bucket_max` per bucket in `VEN/src/common/mod.rs`
- [ ] T005 [US3] Update all 4 `resample_uniform()` call sites in `VEN/src/controller/planner.rs` to pass `Aggregation::Mean` and add `Aggregation` to the import
- [ ] T006 [P] [US3] Update all 8 existing `resample_uniform` test calls in `VEN/src/common/mod.rs` to pass `mean()` helper
- [ ] T007 [P] [US3] Add unit test `resample_uniform_constant_min_max_equal_mean` — constant series produces identical results for all three modes in `VEN/src/common/mod.rs`

**Checkpoint**: `cargo test common::tests` and `cargo test planner::tests` both green. Aggregation infrastructure complete.

---

## Phase 2: User Story 1 — Accurate Slot Cost Across Tariff Boundaries (Priority: P1)

**Goal**: Verify that time-weighted mean aggregation correctly handles tariff changes within a planner slot.

**Independent Test**: Unit tests for tariff boundary scenarios in `VEN/src/common/mod.rs`.

- [ ] T008 [US1] Verify existing test `resample_uniform_tariff_boundary` covers the RF-06 example (slot [10:55, 11:00) = €0.20, slot [11:00, 11:05) = €0.15) in `VEN/src/common/mod.rs`
- [ ] T009 [US1] Verify existing test `resample_uniform_width_larger_than_range` covers multi-step TWM (3min×5.0 + 2min×7.0 = 5.8) in `VEN/src/common/mod.rs`

**Checkpoint**: RF-06 backlog verification examples confirmed by existing tests.

---

## Phase 3: User Story 2 — Strictest Capacity Limit Per Slot (Priority: P1)

**Goal**: Verify that min aggregation picks the lowest (strictest) capacity limit within each slot.

**Independent Test**: `cargo test common::tests::resample_uniform_min` — min-specific tests pass.

- [ ] T010 [US2] Add unit test `resample_uniform_min_step_mid_bucket_change` — step series 10.0→3.0 at mid-bucket produces min=3.0 in `VEN/src/common/mod.rs`
- [ ] T011 [P] [US2] Add unit test `resample_uniform_max_step_mid_bucket_change` — step series 3.0→10.0 at mid-bucket produces max=10.0 in `VEN/src/common/mod.rs`
- [ ] T012 [P] [US2] Add unit test `resample_uniform_min_linear_ramp` — linear 0→60 ramp, first bucket min=0.0/max=5.0, last bucket min=55.0/max=60.0 in `VEN/src/common/mod.rs`
- [ ] T013 [US2] Add unit test `resample_uniform_min_capacity_limit_across_boundary` — 10kW drops to 5kW at 10:57, bucket [10:55, 11:00) min=5.0, earlier buckets=10.0 in `VEN/src/common/mod.rs`

**Checkpoint**: All min/max aggregation scenarios verified. `cargo test common::tests` — 41 tests pass.

---

## Phase 4: Polish & Cross-Cutting Concerns

- [ ] T014 Run full `cargo test` for VEN crate — all 48 tests pass (41 common + 7 planner)
- [ ] T015 Update project journal at `docs/history/project_journal.md`

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Foundational/US3)**: No dependencies — can start immediately
- **Phase 2 (US1)**: Depends on Phase 1 (T004 specifically — `resample_uniform` must accept Aggregation)
- **Phase 3 (US2)**: Depends on Phase 1 (T003 — `bucket_min`/`bucket_max` must exist)
- **Phase 4 (Polish)**: Depends on all prior phases

### User Story Dependencies

- **US3 (P2)**: Foundation — must complete first (enables US1 and US2)
- **US1 (P1)**: Verification only — no new code beyond US3
- **US2 (P1)**: Adds new tests — can run in parallel with US1

### Within Phase 1

- T001 → T002 → T003 (enum → helper → wrappers)
- T004 depends on T002/T003
- T005 depends on T004
- T006, T007 can run in parallel after T004

### Parallel Opportunities

- T003 (min/max wrappers) after T002
- T006, T007 after T004 (test updates are independent)
- T010, T011, T012 after T003 (min/max test cases are independent)
- US1 (Phase 2) and US2 (Phase 3) can run in parallel after Phase 1

---

## Parallel Example: Phase 1

```bash
# After T004 (resample_uniform updated), launch in parallel:
Task T005: "Update planner call sites in VEN/src/controller/planner.rs"
Task T006: "Update existing test calls in VEN/src/common/mod.rs"
Task T007: "Add constant min/max/mean equality test in VEN/src/common/mod.rs"
```

## Parallel Example: Phase 3

```bash
# After T003 (bucket_min/max exist), launch in parallel:
Task T010: "Min step mid-bucket test in VEN/src/common/mod.rs"
Task T011: "Max step mid-bucket test in VEN/src/common/mod.rs"
Task T012: "Min/max linear ramp test in VEN/src/common/mod.rs"
```

---

## Implementation Strategy

### MVP First (Phase 1 Only)

1. Complete Phase 1: Aggregation enum + updated `resample_uniform`
2. **STOP and VALIDATE**: `cargo test` — all existing tests pass unchanged
3. Aggregation infrastructure is usable by downstream features (RF-05b, RF-05e)

### Incremental Delivery

1. Phase 1 → Aggregation infrastructure ready
2. Phase 2 → Tariff boundary verification confirmed
3. Phase 3 → Capacity limit min-aggregation verified
4. Phase 4 → Full test suite + journal

---

## Notes

- This feature is already implemented (commit `5986556`). Tasks document the work done.
- US1 is pure verification (existing tests already cover the RF-06 examples).
- US2 adds 5 new unit tests for min/max aggregation.
- No BDD scenarios needed — this is internal infrastructure with no user-facing behavior change.
- FR-008 (capacity-limit min in planner) deferred to RF-05b when limits become TimeSeries.
