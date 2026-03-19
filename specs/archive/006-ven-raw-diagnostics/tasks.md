# Tasks: VEN Raw Data Diagnostics Page

**Input**: Design documents from `/specs/006-ven-raw-diagnostics/`
**Prerequisites**: plan.md ✅, spec.md ✅, research.md ✅, data-model.md ✅, contracts/ui-components.md ✅, quickstart.md ✅

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story. BDD feature file is written first (red) per the plan's BDD-First requirement.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1, US2, US3)

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: BDD acceptance skeleton, routing, and shared constants — no user story work can start until BDD feature file exists (red requirement from plan.md).

- [x] T001 Write BDD feature file with all acceptance scenarios (must fail) in `tests/features/ven_ui_raw_diagnostics.feature`
- [x] T002 Add Raw Data route and nav button to `VEN/ui/src/App.tsx`
- [x] T003 [P] Create shared CHART_COLORS constant in `VEN/ui/src/components/raw-diagnostics/colors.ts`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: `DiagnosticCell` wrapper is shared by all three user story cells — MUST be complete before any chart component can be wired together.

**⚠️ CRITICAL**: No user story chart work can begin until this phase is complete.

- [x] T004 Create DiagnosticCell wrapper component (title, refresh button, loading/error states) in `VEN/ui/src/components/raw-diagnostics/DiagnosticCell.tsx`
- [x] T005 Write DiagnosticCell unit tests covering loading state, error state, and refresh button click in `VEN/ui/src/__tests__/DiagnosticCell.test.tsx`

**Checkpoint**: Foundation ready — all three user story phases can now proceed.

---

## Phase 3: User Story 1 — Inspect Simulator State (Priority: P1) 🎯 MVP

**Goal**: Render the Simulator State cell with a line chart sourced from `/sim`, showing per-asset power readings connected by a line. Each asset is a categorical x-tick; grid is prepended from `net_power_w / 1000`.

**Independent Test**: Navigate to `/raw-diagnostics`, click the Simulator State refresh button, verify the chart redraws with current asset power readings. Works with no other cells present.

### Implementation for User Story 1

- [x] T006 [P] [US1] Create SimProfileChart categorical line chart component in `VEN/ui/src/components/raw-diagnostics/SimProfileChart.tsx`
- [x] T007 [US1] Create RawDiagnostics page with Simulator State cell wired to `api.sim()` via `useQuery({ enabled: false })` in `VEN/ui/src/pages/RawDiagnostics.tsx`
- [x] T008 [US1] Write RawDiagnostics page unit tests asserting Simulator State cell renders and refresh button triggers fetch in `VEN/ui/src/__tests__/RawDiagnostics.test.tsx`

**Checkpoint**: US1 complete — Simulator State cell is independently functional. BDD scenario "Sim cell refreshes on button click" should pass.

---

## Phase 4: User Story 2 — Inspect Tariff Data (Priority: P2)

**Goal**: Render the Tariffs cell with a multi-line time-series chart sourced from `/rates` (`api.rates()`), plotting `import_price_eur_kwh`, `export_price_eur_kwh`, and `co2_g_kwh` as three distinct colored series.

**Independent Test**: Click the Tariffs cell refresh button; verify three distinct colored lines appear on the chart, one per price dimension.

### Implementation for User Story 2

- [x] T009 [P] [US2] Create TariffsLineChart multi-line time-series component (three series: import/export price + CO₂) in `VEN/ui/src/components/raw-diagnostics/TariffsLineChart.tsx`
- [x] T010 [US2] Add Tariffs cell to RawDiagnostics page wired to `api.rates()` via `useQuery({ enabled: false })` in `VEN/ui/src/pages/RawDiagnostics.tsx`
- [x] T011 [US2] Extend RawDiagnostics unit tests to cover Tariffs cell render and independent refresh in `VEN/ui/src/__tests__/RawDiagnostics.test.tsx`

**Checkpoint**: US2 complete — Tariffs cell works independently alongside US1. BDD scenario "Tariffs cell refreshes on button click" should pass.

---

## Phase 5: User Story 3 — Browse Historical and Near-Future Timeline (Priority: P3)

**Goal**: Render the Timeline cell with a series dropdown (populated from response keys at fetch time, default "grid") and a time-series line chart sourced from `/timeline/all?hours_back=1.0&hours_forward=1.0`. Only the selected series is plotted.

**Independent Test**: Select "grid" from the Timeline dropdown, click refresh; verify the chart shows grid `power_kw` readings over the ±1 hour window. Changing the dropdown selection and refreshing replaces the chart data.

### Implementation for User Story 3

- [x] T012 [P] [US3] Create TimelineSeriesChart component with MUI Select series dropdown and single-series line chart in `VEN/ui/src/components/raw-diagnostics/TimelineSeriesChart.tsx`
- [x] T013 [US3] Add Timeline cell to RawDiagnostics page wired to `api.allTimelines({ hours_back: 1.0, hours_forward: 1.0 })` via `useQuery({ enabled: false })`, managing `selectedSeries` state in `VEN/ui/src/pages/RawDiagnostics.tsx`
- [x] T014 [US3] Extend RawDiagnostics unit tests to cover Timeline cell series dropdown and chart render in `VEN/ui/src/__tests__/RawDiagnostics.test.tsx`

**Checkpoint**: All three user stories complete — page renders three stacked cells, each independently refreshable. All BDD scenarios except deployment-dependent ones should pass in Vitest.

---

## Phase 6: Polish & Verification

**Purpose**: BDD step definitions and Pi4 deployment to close the acceptance loop.

- [x] T015 Write BDD step definitions covering all scenarios from `ven_ui_raw_diagnostics.feature` in `tests/steps/ven_ui_raw_diagnostics_steps.py`
- [x] T016 Deploy to Pi4-Server (`git push` → `ssh Pi4-Server "cd /srv/docker/openadr_lab && git pull && docker compose build ven-ui && docker compose up -d ven-ui"`) and run BDD tests green: `docker compose -f tests/docker-compose.test.yml run --build --rm test-runner features/ven_ui_raw_diagnostics.feature`

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately; T002 and T003 can run in parallel after T001
- **Foundational (Phase 2)**: Depends on Phase 1 completion — BLOCKS all user story phases
- **User Stories (Phases 3–5)**: All depend on Phase 2 completion; can proceed sequentially P1→P2→P3
- **Polish (Phase 6)**: Depends on all user story phases complete

### User Story Dependencies

- **US1 (P1)**: Can start after Foundational — no dependency on US2 or US3
- **US2 (P2)**: Can start after Foundational — no dependency on US1 or US3 (adds a cell to the page)
- **US3 (P3)**: Can start after Foundational — no dependency on US1 or US2 (adds a cell to the page)

### Within Each User Story

- Chart component [P] tasks (T006, T009, T012) can be created independently of page wiring
- Page wiring tasks (T007/T010/T013) depend on the chart component being available
- Test tasks (T008/T011/T014) depend on page wiring being complete

### Parallel Opportunities

- T002 + T003 after T001 (different files)
- T006, T009, T012 can all be created in parallel (each is a separate chart component file)
- T004 (DiagnosticCell) can be started as soon as T003 (colors.ts) is done

---

## Parallel Example: Chart Components

```
Once Phase 2 is done, all three chart components can be built in parallel:

  T006: SimProfileChart.tsx       (categorical line)
  T009: TariffsLineChart.tsx      (multi-line time-series)
  T012: TimelineSeriesChart.tsx   (dropdown + single-series)
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup (T001–T003)
2. Complete Phase 2: Foundational (T004–T005)
3. Complete Phase 3: US1 Simulator State (T006–T008)
4. **STOP and VALIDATE**: Navigate to `/raw-diagnostics`, click Sim refresh, verify chart renders
5. Deploy MVP if ready

### Incremental Delivery

1. Setup + Foundational → skeleton page with DiagnosticCell wired
2. Add US1 (Sim cell) → test independently → deploy
3. Add US2 (Tariffs cell) → test independently → deploy
4. Add US3 (Timeline cell + dropdown) → test independently → deploy
5. Write BDD steps → deploy → run full BDD suite green

---

## Notes

- [P] tasks = different files, no shared state dependencies
- [Story] label maps each task to its user story for traceability
- BDD feature file (T001) must exist and fail before any implementation — this is the BDD-First gate from plan.md
- `useQuery({ enabled: false })` + `refetch()` is the mandatory manual-refresh pattern (React Guidelines)
- `/tariffs` is accessed via `api.rates()` in VenApi — this is a legacy name; do not rename
- All interactive elements must have `data-testid` per React Guidelines
- `--build` is always required when running BDD test-runner after source changes
