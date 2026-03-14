# Tasks: VEN Controller Dashboard V2

**Input**: Design documents from `/specs/001-controller-dashboard-v2/`
**Prerequisites**: plan.md ✅, spec.md ✅, research.md ✅, data-model.md ✅, contracts/ui-components.md ✅

**Organization**: Tasks are grouped by user story. Each phase is independently testable and deliverable.

**BDD-first**: All feature files are written in Phase 1 and must FAIL before any implementation begins (Constitution Principle II).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no shared dependencies)
- **[Story]**: Which user story this task belongs to (US1–US5)

---

## Phase 1: Setup — BDD Scenarios + Routing

**Purpose**: Write all BDD scenarios first (red phase), add the new route. No implementation code until T006.

**⚠️ CONSTITUTION**: Feature files MUST be written and confirmed failing before Phase 2 begins.

- [X] T001 [P] Write `tests/features/controller_v2/01_layout.feature` — scenarios: grid cells appear above asset cells, page is scrollable, grid cells scroll with page (not fixed)
- [X] T002 [P] Write `tests/features/controller_v2/02_asset_cells.feature` — scenarios: left section shows power/cost/CO₂, NOW line visible, solid/dashed/dotted lines, negative power renders below x-axis
- [X] T003 [P] Write `tests/features/controller_v2/03_simulation_controls.feature` — scenarios: EV plugged toggle visible, SoC slider visible, POST /sim/override triggered on change
- [X] T004 [P] Write `tests/features/controller_v2/04_navigation.feature` — scenarios: pin cell → stays in viewport while scrolling, unpin → returns to position, collapse left section, collapse right section
- [X] T005 Run test-runner on Pi4-Server to confirm all 4 feature files fail: `docker compose -f tests/docker-compose.test.yml run --build --rm test-runner features/controller_v2/`
- [X] T006 Add `/controller-v2` route and "Controller V2" nav link to `VEN/ui/src/App.tsx` — import placeholder `ControllerV2` page (stub returning `<div>Controller V2</div>`)

**Checkpoint**: All 4 feature files exist and fail. Route `/controller-v2` is reachable (shows stub).

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Shared types, data builder stubs, and backend stubs that every user story component depends on.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete.

- [X] T007 Add 3 stub fields to `UserOverrides` struct in `VEN/src/state.rs`: `ev_initial_soc: Option<f64>`, `battery_initial_soc: Option<f64>`, `battery_capacity_kwh: Option<f64>`; apply one-shot / persistent semantics in the simulator tick handler
- [ ] T008 Rebuild and redeploy VEN backend on Pi4-Server after `state.rs` change: `docker compose build ven-ven-1 && docker compose up -d ven-ven-1`
- [X] T009 Create `VEN/ui/src/components/controller-v2/types.ts` — define: `AssetId`, `AssetSummary`, `AssetTimePoint`, `TariffSnapshot`, `TariffTimePoint`, `StackedAreaPoint`, `PinnedState`, `CollapseState` (per data-model.md)
- [X] T010 Create stub `VEN/ui/src/components/controller-v2/dataBuilders.ts` — declare all 5 functions from contracts/ui-components.md with empty return values: `buildAssetTimeline()`, `buildStackedAreaData()`, `buildTariffTimeline()`, `findCurrentTariff()`, `deriveAssetSummaries()`

**Checkpoint**: Backend stub fields are live. Types and data builder signatures are declared. All user story phases can now proceed (potentially in parallel if staffed).

---

## Phase 3: User Story 1 — Monitor Asset Real-Time Status (P1) 🎯 MVP

**Goal**: Every asset cell shows current power [kW], cost rate [€/h], CO₂eq rate [g CO₂eq/h], and SoC where applicable. Operator can see values update live.

**Independent Test**: Load `/controller-v2`, verify each asset cell's left section displays labeled power/cost/CO₂ values with correct units and sign convention, without requiring any graph or controls to work.

- [ ] T011 [US1] Implement `deriveAssetSummaries()` in `VEN/ui/src/components/controller-v2/dataBuilders.ts` — reads `SimSnapshot` + tariff intervals + `UserRequest[]` + `Plan | null` → returns `AssetSummary[]` per data-model.md; includes `findCurrentTariff()` helper
- [ ] T012 [US1] Implement `AssetLeftSection` in `VEN/ui/src/components/controller-v2/AssetLeftSection.tsx` — displays power, cost rate, CO₂eq rate, SoC (if applicable), forecast energy (if plan data available), active user request (if any); all `data-testid` attributes per contracts/ui-components.md
- [ ] T013 [US1] Implement `AssetCell` shell in `VEN/ui/src/components/controller-v2/AssetCell.tsx` — three-section horizontal layout (left / mid placeholder / right placeholder); wires `AssetLeftSection`; collapse toggles for left and right; pin button; all `data-testid` attributes
- [ ] T014 [US1] Implement `ControllerV2` page in `VEN/ui/src/pages/ControllerV2.tsx` — calls `useSim()`, `useRates()`, `useUserRequests()`, `usePlan()`; detects present assets from `SimSnapshot`; derives `AssetSummary[]` via `deriveAssetSummaries()`; renders one `AssetCell` per asset in vertical stack; initialises `pinnedCellIds` and `collapseState` useState
- [ ] T015 [US1] Rebuild VEN UI on Pi4-Server and verify US1 BDD scenarios in `02_asset_cells.feature` (left-section assertions) pass

**Checkpoint**: Each asset cell shows live power/cost/CO₂ values. Mid and right sections are visible as placeholders. US1 acceptance scenarios pass.

---

## Phase 4a: User Story 2 — View Asset Energy Timeline (P2)

**Goal**: Each asset cell's center section shows a time-series graph with past (solid) and future (dashed/dotted) data, a red "NOW" line, and three line types for power/cost/CO₂eq.

**Independent Test**: With simulation running, verify the mid section graph renders trace data to the left of the NOW line and plan allocations to the right, using the correct line styles, without the right section controls needing to work.

- [ ] T016 [US2] Implement `buildAssetTimeline()` in `VEN/ui/src/components/controller-v2/dataBuilders.ts` — merges `TraceEntry[]` (past, per-asset setpoints) + `Plan` firm/flexible slot allocations (future, per-asset `power_kw`) + `RateSnapshot[]` (tariffs) → `AssetTimePoint[]`; marks each point `isPast`; derives `costRateEurH` and `co2RateGH` per point via `findCurrentTariff()`
- [ ] T017 [US2] Implement `AssetTimelineChart` in `VEN/ui/src/components/controller-v2/charts/AssetTimelineChart.tsx` — recharts `ComposedChart`; power solid line, cost rate dashed (`strokeDasharray="5 5"`), CO₂eq rate dotted (`strokeDasharray="2 2"`); all lines share asset color; dual Y-axes (power left, rates right); `ReferenceLine` at `nowMs` red dotted; `connectNulls={false}`; legend with units; `data-testid="asset-timeline-chart-{assetId}"`
- [ ] T018 [US2] Implement `AssetMidSection` in `VEN/ui/src/components/controller-v2/AssetMidSection.tsx` — `ResponsiveContainer` wrapping `AssetTimelineChart`; receives `assetId`, `timePoints`, `color`; `data-testid="asset-cell-{assetId}-mid"`
- [ ] T019 [US2] Wire `AssetMidSection` into `AssetCell` — add `timePoints: AssetTimePoint[]` and `color: string` props to `AssetCell`; pass from `ControllerV2.tsx` using `buildAssetTimeline()` (add `useTrace(500)` hook call to page)
- [ ] T020 [US2] Rebuild VEN UI on Pi4-Server and verify US2 BDD scenarios in `02_asset_cells.feature` (graph assertions) pass

**Checkpoint**: Asset timeline graphs show past trace + planned future with correct line styles and NOW marker. US2 acceptance scenarios pass.

---

## Phase 4b: User Story 4 — Monitor Grid Tariffs and Power Balance (P2)

**Goal**: Two grid cells at the top of the page show current tariff values and a stacked asset power breakdown. Can be worked in parallel with Phase 4a.

**Independent Test**: Load the dashboard with at least two active assets. Verify the Tariff Cell left section shows import/export tariff and derived rates, and the Accumulated Asset Power Cell stacked area chart shows each asset as a colored area (positive above x-axis, negative below), summing to grid power.

- [ ] T021 [US4] Implement `buildTariffTimeline()` and `buildStackedAreaData()` in `VEN/ui/src/components/controller-v2/dataBuilders.ts` — tariff timeline merges `RateSnapshot[]` intervals + trace net power (past) + plan `net_import_kw` (future) → `TariffTimePoint[]`; stacked area data splits each asset power into `_pos`/`_neg` per time point → `StackedAreaPoint[]`
- [ ] T022 [P] [US4] Implement `TariffChart` in `VEN/ui/src/components/controller-v2/charts/TariffChart.tsx` — 5 series: import tariff [€/kWh] red dashed, import CO₂eq tariff [g CO₂eq/kWh] red dotted, export tariff [€/kWh] green dashed, total cost rate [€/h] black dashed, grid power [kW] black solid; `ReferenceLine` at `nowMs`; `data-testid="tariff-chart"`
- [ ] T023 [P] [US4] Implement `StackedAreaChart` in `VEN/ui/src/components/controller-v2/charts/StackedAreaChart.tsx` — recharts `AreaChart`; for each asset render `{id}_pos` Area with `stackId="positive"` and `{id}_neg` Area with `stackId="negative"`; asset colors from `colorMap`; `ReferenceLine` at `nowMs`; `data-testid="accumulated-area-chart"`
- [ ] T024 [P] [US4] Implement `GridTariffCell` in `VEN/ui/src/components/controller-v2/GridTariffCell.tsx` — left section: 5 labeled values from `TariffSnapshot` (import tariff, import CO₂eq tariff, export tariff, total cost rate, grid power); right section: `TariffChart`; pin button; no controls; all `data-testid` attributes per contracts/ui-components.md
- [ ] T025 [P] [US4] Implement `GridAccumulatedCell` in `VEN/ui/src/components/controller-v2/GridAccumulatedCell.tsx` — left section: list of all assets with current power [kW] from `AssetSummary[]`; right section: `StackedAreaChart`; pin button; no controls; all `data-testid` attributes
- [ ] T026 [US4] Wire grid cells into `ControllerV2.tsx` — render `GridTariffCell` and `GridAccumulatedCell` above asset cells using `buildTariffTimeline()`, `buildStackedAreaData()`, and a derived `TariffSnapshot` from current `/sim` + `/rates`
- [ ] T027 [US4] Rebuild VEN UI on Pi4-Server and verify US4 BDD scenarios in `01_layout.feature` (grid cell ordering and content) pass

**Checkpoint**: Both grid cells visible above asset cells, showing live tariff data and stacked area power breakdown. US4 acceptance scenarios pass.

---

## Phase 5: User Story 3 — Simulate Asset Behavior via Controls (P3)

**Goal**: Right section of each asset cell shows asset-appropriate controls. Changes POST to `/sim/override` and take effect within one sim tick.

**Independent Test**: Toggle the EV plugged-in switch. Verify the EV power value in the left section updates to 0 (or charging rate) within 5 seconds, and the POST to `/sim/override` carries the updated `ev_plugged` value.

- [ ] T028 [US3] Implement `AssetRightSection` in `VEN/ui/src/components/controller-v2/AssetRightSection.tsx` — two MUI Accordion groups: **Status Settings** (expanded by default — SoC slider for EV/Battery using stub fields, power on/off toggle) and **Simulation Characteristics** (collapsed by default — capacity, power limits, temp range, delays per asset type); read initial values from `SimSnapshot` + `UserOverrides`; POST changes via read-current-merge-write pattern using `useSetSimOverride()`; all `data-testid` attributes per contracts/ui-components.md
- [ ] T029 [US3] Wire `AssetRightSection` into `AssetCell` — add `simSnapshot`, `overrides`, `onOverrideChange` props; call `useSimOverride()` and `useSetSimOverride()` in `ControllerV2.tsx`; pass down through `AssetCell`
- [ ] T030 [US3] Rebuild VEN UI on Pi4-Server and verify US3 BDD scenarios in `03_simulation_controls.feature` pass

**Checkpoint**: Each asset cell right section shows live-readable controls. EV plugged toggle works end-to-end. US3 acceptance scenarios pass.

---

## Phase 6: User Story 5 — Navigate and Customize Dashboard Layout (P3)

**Goal**: Operator can pin cells (they stay fixed at top while scrolling) and collapse left/right sections of asset cells.

**Independent Test**: On a page with 4+ cells, pin one cell, scroll down. The pinned cell remains at the top of the viewport. Unpin — it returns to its natural position. Collapse the left section of an asset cell — only graph and right section remain visible.

- [ ] T031 [US5] Implement `PinnedZone` in `VEN/ui/src/components/controller-v2/PinnedZone.tsx` — sticky `position: sticky; top: 0` or `position: fixed` container rendering all currently pinned cells; `data-testid="pinned-zone"`
- [ ] T032 [US5] Add pin/collapse state and handlers to `ControllerV2.tsx` — `pinnedCellIds: string[]` state; `handleTogglePin(cellId)` adds/removes from array; `collapseState: CollapseState` state; `handleToggleCollapse(cellId, section)` flips boolean; thread all handlers down to cell components
- [ ] T033 [US5] Wire pin toggle into `AssetCell`, `GridTariffCell`, `GridAccumulatedCell` — show pin icon button; call `onTogglePin`; move pinned cells into `PinnedZone` in `ControllerV2.tsx` render
- [ ] T034 [US5] Wire collapse toggles into `AssetCell` — left and right section collapse buttons call `onToggleCollapse`; wrap each section in MUI `Collapse`; collapse icon rotates to indicate state
- [ ] T035 [US5] Rebuild VEN UI on Pi4-Server and verify US5 BDD scenarios in `04_navigation.feature` pass

**Checkpoint**: Pin/collapse/unpin all work end-to-end. Page scroll is smooth with many cells. US5 acceptance scenarios pass.

---

## Phase 7: Polish & Cross-Cutting Concerns

**Purpose**: Unit test coverage, data-testid audit, full test suite validation, documentation.

- [ ] T036 Write `VEN/ui/src/__tests__/ControllerV2.test.tsx` — mock all hooks (`vi.mock("../api/hooks", ...)`); test: page renders without crash, asset cells present for each mocked asset, grid cells present, pin button present, left/right collapse buttons present; use `data-testid` selectors
- [ ] T037 Audit `data-testid` coverage across all new components against `contracts/ui-components.md` — every listed `data-testid` must exist in the corresponding component
- [ ] T038 Run full BDD test suite on Pi4-Server — zero failures required: `docker compose -f tests/docker-compose.test.yml run --build --rm test-runner`
- [ ] T039 Update `docs/history/project_journal.md` — record what was built, key decisions, issues encountered; update `docs/reference/KEY_LEARNINGS.md` with any new lessons

**Checkpoint**: All 39 tasks complete. Zero test failures. Documentation updated. Feature ready for review.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Setup)**: No dependencies — start immediately
- **Phase 2 (Foundation)**: Depends on Phase 1 complete — blocks all user story phases
- **Phase 3 (US1 P1)**: Depends on Phase 2 — start here, this is the MVP
- **Phase 4a (US2 P2)**: Depends on Phase 3 (needs ControllerV2.tsx shell + AssetCell structure)
- **Phase 4b (US4 P2)**: Depends on Phase 2 only — can run in parallel with Phase 4a
- **Phase 5 (US3 P3)**: Depends on Phase 3 (needs AssetCell shell)
- **Phase 6 (US5 P3)**: Depends on Phase 3 (needs ControllerV2.tsx state + cells)
- **Phase 7 (Polish)**: Depends on all phases complete

### User Story Dependencies

```
Phase 1 → Phase 2 → Phase 3 (US1) ─┬─→ Phase 4a (US2)
                                    ├─→ Phase 5  (US3)
                                    └─→ Phase 6  (US5)
Phase 2 ──────────────────────────────→ Phase 4b (US4)
```

### Parallel Opportunities Within Phases

- **T001–T004**: All 4 BDD feature files can be written simultaneously
- **T009–T010**: types.ts and dataBuilders.ts stub can be written simultaneously
- **T022–T025**: TariffChart, StackedAreaChart, GridTariffCell, GridAccumulatedCell have no mutual dependencies

---

## Parallel Execution Examples

### Phase 1 — all BDD files at once
```
Task T001: Write 01_layout.feature
Task T002: Write 02_asset_cells.feature
Task T003: Write 03_simulation_controls.feature
Task T004: Write 04_navigation.feature
```

### Phase 4b — charts and cells at once
```
Task T022: Implement TariffChart
Task T023: Implement StackedAreaChart
Task T024: Implement GridTariffCell
Task T025: Implement GridAccumulatedCell
```

---

## Implementation Strategy

### MVP First (US1 only — Phases 1–3)

1. Phase 1: Write all BDD files, confirm they fail
2. Phase 2: Backend stubs + shared types
3. Phase 3: US1 — asset left-section metrics visible live
4. **STOP and VALIDATE**: left-section values update, sign convention correct, cost/CO₂ derivation correct
5. Deploy and demo to stakeholder

### Incremental Delivery

1. Phases 1–3 → MVP: live asset metrics per cell
2. Phase 4a → Add timelines: past + future graphs per asset
3. Phase 4b → Add grid overview: tariff cell + stacked power cell
4. Phase 5 → Add simulation controls: right section interactive
5. Phase 6 → Add layout controls: pin and collapse
6. Phase 7 → Polish, tests, journal

---

## Notes

- `[P]` tasks touch different files — safe to implement simultaneously
- Constitution Principle II: feature files (T001–T004) MUST fail (T005) before T007 begins
- `POST /sim/override` is full-replace — always read current overrides and merge before posting (implemented in T028)
- Run vitest from `/c/DriveD/Tinker/OpenAdr-Lab/VEN/ui` (real path, not subst drive) — see KEY_LEARNINGS.md
- After any VEN Rust source change, rebuild image explicitly before testing
- After any feature file or step definition change, always pass `--build` to test-runner
