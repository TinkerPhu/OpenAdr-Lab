# Tasks: Grid-Aligned UI Timeline

**Input**: Design documents from `/specs/011-grid-aligned-ui/`
**Prerequisites**: plan.md (required), spec.md (required), research.md, data-model.md

**Tests**: Unit test updates included as part of implementation (existing test infrastructure, no new test framework setup needed).

**Organization**: Tasks are grouped by user story to enable independent implementation and testing.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

## Phase 1: Foundational (Type Change)

**Purpose**: Update the shared `AssetTimelinePoint` type to accept `values: null`. This unblocks all user stories.

- [x] T001 Update `AssetTimelinePoint.values` type from `Record<string, number>` to `Record<string, number> | null` in `VEN/ui/src/components/controller-v2/types.ts`

**Checkpoint**: Type change compiles. All downstream files will now show TypeScript errors where `values` is accessed without null-check — these are addressed in the user story phases.

---

## Phase 2: User Story 1 - Accurate Stacked Area Chart (Priority: P1)

**Goal**: Replace tolerance-based `findNearest`/`TOLERANCE_MS` in `GridAccumulatedCell` with positional zip across asset arrays.

**Independent Test**: Run `npm test -- GridAccumulatedCell` — all tests pass with new positional-zip logic and null-values handling.

### Implementation for User Story 1

- [x] T002 [US1] Remove `findNearest()` function (lines 17-32) and `TOLERANCE_MS` constant (line 51) from `VEN/ui/src/components/controller-v2/GridAccumulatedCell.tsx`
- [x] T003 [US1] Replace `buildStackedFromAllTimelines()` with positional-zip implementation in `VEN/ui/src/components/controller-v2/GridAccumulatedCell.tsx` — iterate by shared index across all asset arrays, handle `values: null` as zero contribution, extract `gridPowerKw` from `allTimelines["grid"][i]`
- [x] T004 [US1] Update `VEN/ui/src/__tests__/GridAccumulatedCell.test.tsx` — update mock for `useAllTimelines` to return grid-aligned data; add test cases for positional-zip (normal data, empty data, null-values entries)

**Checkpoint**: Stacked area chart uses positional indexing. `findNearest` and `TOLERANCE_MS` are gone. Tests pass.

---

## Phase 3: User Story 2 - All Asset Cell Charts Handle Null Values (Priority: P1)

**Goal**: Ensure per-asset charts and data builders handle `values: null` entries without crashing.

**Independent Test**: Run `npm test -- dataBuilders` and visually verify asset cell charts render with null-values data.

### Implementation for User Story 2

- [x] T005 [P] [US2] Add optional chaining for `values` access in all 3 `dataKey` accessors in `VEN/ui/src/components/controller-v2/charts/AssetTimelineChart.tsx` — change `pt.values["key"]` to `pt.values?.["key"]`
- [x] T006 [P] [US2] Update `computeForecastEnergy` in `VEN/ui/src/components/controller-v2/dataBuilders.ts` to skip entries where `values` is `null` (add null-check before accessing `values["power_kw"]`)
- [x] T007 [P] [US2] Add null-values test case to `VEN/ui/src/__tests__/dataBuilders.test.ts` — verify `forecastFor` returns correct result when timeline contains `{ts, values: null}` entries

**Checkpoint**: Asset cell charts and forecast energy calculations handle null values. All asset cell tests pass.

---

## Phase 4: User Story 3 - Grid Tariff Cell Handles Null Values (Priority: P1)

**Goal**: Ensure `buildPowerPoints` handles `values: null` entries from the grid timeline.

**Independent Test**: Run `npm test -- GridTariffCell` — tariff cell tests pass with null-values data.

### Implementation for User Story 3

- [x] T008 [US3] Update `buildPowerPoints` in `VEN/ui/src/components/controller-v2/tariffBuilders.ts` to handle `values: null` — produce `TariffTimePoint` with `gridPowerKw: null` and `totalCostRateEurH: null` for null-values entries

**Checkpoint**: Tariff cell grid power line handles empty grid buckets.

---

## Phase 5: User Story 4 - Clean Codebase (Priority: P2)

**Goal**: Verify all dead code is removed and no references to `findNearest` or `TOLERANCE_MS` remain.

**Independent Test**: Grep the codebase for `findNearest` and `TOLERANCE_MS` — zero results.

- [x] T009 [US4] Verify no references to `findNearest` or `TOLERANCE_MS` remain anywhere in `VEN/ui/src/` — grep and fix any remaining occurrences

**Checkpoint**: Codebase is clean. No dead nearest-neighbour code.

---

## Phase 6: User Story 5 - Resolution Query Parameter (Priority: P3)

**Goal**: API client supports `resolution` parameter, `maxPoints` kept as deprecated alias.

**Independent Test**: Inspect `api.allTimelines({ resolution: 30 })` call to verify URL contains `resolution=30`.

### Implementation for User Story 5

- [x] T010 [P] [US5] Add `resolution` option to `allTimelines()` method in `VEN/ui/src/api/client.ts` — set `resolution` query param when provided; keep existing `maxPoints` as fallback
- [x] T011 [P] [US5] Update `useAllTimelines` hook in `VEN/ui/src/api/hooks.ts` to accept and pass `resolution` option

**Checkpoint**: API client sends `resolution` parameter. Backward-compatible with `maxPoints`.

---

## Phase 7: Polish & Cross-Cutting Concerns

**Purpose**: Handle remaining consumers and run full validation.

- [x] T012 [P] Update `RawDiagnostics` type annotation in `VEN/ui/src/pages/RawDiagnostics.tsx` — the `useQuery<Record<string, AssetTimelinePoint[]>>` generic may need updating if `AssetTimelinePoint` change requires it
- [x] T013 [P] Update empty-data fallback in `AssetTimelineChart` (line 39) — change `values: {}` to remain compatible with the nullable type (already valid, verify no TS error)
- [x] T014 Run full `npm test` in `VEN/ui/` to verify all vitest tests pass
- [ ] T015 Run quickstart.md validation — verify Controller V2 page renders correctly with live backend

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Foundational)**: No dependencies — start immediately
- **Phase 2-4 (US1, US2, US3)**: All depend on Phase 1 (type change). US2 and US3 can run in parallel with US1.
- **Phase 5 (US4)**: Depends on Phase 2 (findNearest removal happens there)
- **Phase 6 (US5)**: Independent of all other user stories — can run in parallel after Phase 1
- **Phase 7 (Polish)**: Depends on all previous phases

### User Story Dependencies

- **US1 (Stacked Area)**: Depends on T001 only. Core change.
- **US2 (Asset Cells)**: Depends on T001 only. Can run in parallel with US1.
- **US3 (Tariff Cell)**: Depends on T001 only. Can run in parallel with US1 and US2.
- **US4 (Clean Codebase)**: Depends on US1 completion (T002 removes the code).
- **US5 (Resolution Param)**: Depends on T001 only. Fully independent.

### Parallel Opportunities

After T001 (type change), these can all run in parallel:
- T002+T003 (US1 stacked area rewrite)
- T005+T006+T007 (US2 asset cells — all [P] marked)
- T008 (US3 tariff cell)
- T010+T011 (US5 resolution param — all [P] marked)

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete T001 (type change)
2. Complete T002-T004 (stacked area positional zip)
3. **STOP and VALIDATE**: `npm test -- GridAccumulatedCell` passes, visual check on Controller V2

### Incremental Delivery

1. T001 → Foundation ready
2. T002-T004 → Stacked area chart fixed (MVP)
3. T005-T007 → Asset cells handle nulls
4. T008 → Tariff cell handles nulls
5. T009 → Dead code verified removed
6. T010-T011 → Resolution parameter
7. T012-T015 → Polish and full validation

---

## Notes

- Total: 15 tasks
- The type change (T001) is the single foundation task — everything else depends on it
- Most work is in US1 (T002-T004) — the positional-zip rewrite
- US2-US3 are defensive null-handling — small changes (optional chaining, null checks)
- US5 is fully independent and low-risk
- No new files created — all edits to existing files
