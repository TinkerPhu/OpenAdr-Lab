# Tasks: Planner Visualization Page

**Input**: Design documents from `/specs/014-planner-viz-page/`
**Prerequisites**: plan.md ✅, spec.md ✅, research.md ✅, data-model.md ✅, quickstart.md ✅

**Tests**: Included — spec SC-008 mandates 100% automated coverage of all introduced UI behaviors.
Tests are written FIRST (red), then implementation (green). BDD scenarios are written before any component code.

**Organization**: Tasks grouped by user story to enable independent, incremental delivery.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no shared dependencies)
- **[Story]**: Which user story this task belongs to (US1–US4)

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Create the directory skeleton and confirm the build passes before writing any logic.

- [x] T001 Create component directory `VEN/ui/src/components/planner/` (empty — placeholder for Phase 3–6 component files)
- [x] T002 [P] Verify `npm test` and `npm run build` pass cleanly on current codebase before any changes (baseline green)

**Checkpoint**: Clean baseline confirmed — all existing tests pass.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Type extensions, BDD scaffold, and route stub that MUST exist before any user story component work.

**⚠️ CRITICAL**: No user story component work begins until all Foundation tasks are complete.

### Type Extensions

- [x] T003 Add `PlanReason` discriminated union type (12 variants with parameters) to `VEN/ui/src/api/types.ts` — see data-model.md for exact field names
- [x] T004 Add `PlanStep` type to `VEN/ui/src/api/types.ts` — fields: `ts`, `asset_id`, `setpoint_kw`, `actual_power_kw`, `reason: PlanReason`, `state_before`, `avail_max_import_kw`, `avail_max_export_kw`
- [x] T005 Extend `Plan` type in `VEN/ui/src/api/types.ts` — add `firm_boundary: string`, `steps: PlanStep[]`, and `suggested_action: string | null` to each warnings array element

### BDD Scaffold (write first, confirm red)

- [x] T006 Write complete BDD feature file `tests/features/ven_ui_planner.feature` with all `@ven-ui` scenarios from plan.md Phase B — Navigation, Plan Header, Trigger Timeline, Decision Matrix, Packet Board sections
- [x] T007 Write BDD step stubs in `tests/features/steps/planner_ui_steps.py` — one stub per step used in T006; all stubs call `context.scenario.skip()` or assert False initially to confirm red
- [x] T008 Run BDD to confirm feature file is syntactically valid and all scenarios are "red" (unimplemented): `docker compose -f tests/docker-compose.test.yml run --build --rm test-runner features/ven_ui_planner.feature`

### Route Stub

- [x] T009 Add `PlannerPage` import, `/planner` route, and "Planner" nav button (`data-testid="nav-planner"`) to `VEN/ui/src/App.tsx` — position nav button between "Controller V2" and "User Requests"
- [x] T010 Create `VEN/ui/src/pages/Planner.tsx` as a stub page (renders heading `data-testid="planner-heading"` and four `TODO` section placeholders; calls `usePlan()`, `useTrace(20)`, `usePackets()`)

**Checkpoint**: `npm run build` succeeds with new types; `/planner` route exists and shows stub; BDD scenarios are red.

---

## Phase 3: User Story 1 — Decision Matrix (Priority: P1) 🎯 MVP

**Goal**: Engineers can open the Planner page and see every asset-slot planning decision as a color-coded cell, click any cell for full PlanStep detail, and identify the FIRM/FLEX boundary.

**Independent Test**: With a live VEN plan, navigate to `/planner` → Decision Matrix renders colored cells → click any cell → drawer shows reason type and parameters. Confirm `npm test` passes PlanDecisionMatrix unit tests.

### Tests for User Story 1 ⚠️ Write FIRST, confirm FAIL before implementation

- [x] T011 [P] [US1] Write vitest unit tests in `VEN/ui/src/__tests__/PlanDecisionMatrix.test.tsx` covering:
  - Empty state when `plan` is null (renders `data-testid="matrix-empty"`)
  - Renders one row per unique `asset_id` in `plan.steps`
  - Cell has correct `bgcolor` based on `PlanReason` type (check `data-testid="matrix-cell-ev-0"` style)
  - FIRM cells have full opacity; FLEXIBLE cells have `opacity: 0.5` style
  - FIRM/FLEX divider element exists (`data-testid="matrix-firm-flex-divider"`)
  - Click cell → drawer opens (`data-testid="matrix-drawer"` is visible)
  - Drawer shows reason type text in `data-testid="matrix-drawer-reason"`
  - Collapse button hides cells; expand button shows them
  - Expand-horizon button adds FLEXIBLE columns (column count increases)
  - Default view shows only FIRM columns

### Implementation for User Story 1

- [x] T012 [US1] Implement `REASON_META` lookup constant in `VEN/ui/src/components/planner/PlanDecisionMatrix.tsx` — maps each PlanReason type to `{ label, color, icon, title }` per data-model.md
- [x] T013 [US1] Implement matrix data derivation in `PlanDecisionMatrix.tsx`: group `plan.steps` by `asset_id`, sort unique assets alphabetically, sort by `ts`, build `MatrixCell[][]` (rows=assets, cols=timeslots); join `plan.firm_slots + plan.flexible_slots` to get `import_tariff_eur_kwh` per column
- [x] T014 [US1] Implement tariff header row in `PlanDecisionMatrix.tsx`: interpolate `import_tariff_eur_kwh` from [min,max] range to CSS color scale green→yellow→red; one `Box` per column with `bgcolor`
- [x] T015 [US1] Implement FIRM/FLEX boundary in `PlanDecisionMatrix.tsx`: find column index where `ts >= plan.firm_boundary`; render `data-testid="matrix-firm-flex-divider"` `Box` at that column; apply `opacity: 0.5` to all FLEXIBLE columns
- [x] T016 [US1] Implement asset cell rows in `PlanDecisionMatrix.tsx`: one `Box` per `MatrixCell` with `bgcolor` from `REASON_META[step.reason.type].color`, `title` tooltip from `REASON_META.title`, `data-testid="matrix-cell-{assetId}-{slotIndex}"`, `cursor: pointer`
- [x] T017 [US1] Implement step detail drawer in `PlanDecisionMatrix.tsx`: MUI `Drawer` anchored right, `data-testid="matrix-drawer"`; on cell click store selected `PlanStep`; render ts, asset_id, setpoint_kw, actual_power_kw, state_before, reason type + parameters (branch on `step.reason.type` for each variant); `data-testid="matrix-drawer-reason"` on reason section
- [x] T018 [US1] Implement collapse/expand section button (`data-testid="matrix-collapse-btn"`) and expand-horizon button (`data-testid="matrix-expand-horizon-btn"`) in `PlanDecisionMatrix.tsx`; default view shows only FIRM columns
- [x] T019 [US1] Implement reason legend in `PlanDecisionMatrix.tsx`: static MUI `Stack` below grid showing all 12 `REASON_META` entries with color swatch, icon, and label
- [x] T020 [US1] Implement empty state in `PlanDecisionMatrix.tsx`: when `plan` is null/undefined, show MUI `Typography` with `data-testid="matrix-empty"` and "No plan available" text
- [x] T021 [US1] Implement BDD step definitions in `tests/features/steps/planner_ui_steps.py` for Decision Matrix scenarios: navigate-to-planner, assert-matrix-visible, assert-cell-visible, click-cell, assert-drawer-open, assert-drawer-reason, assert-divider-visible, collapse-matrix, expand-matrix
- [x] T022 [US1] Verify all Decision Matrix vitest tests pass (`npm test -- PlanDecisionMatrix`) and BDD Decision Matrix scenarios pass on Pi4

**Checkpoint**: US1 is independently functional. Navigate to `/planner` → Decision Matrix works end-to-end.

---

## Phase 4: User Story 2 — Packet Progress Board (Priority: P2)

**Goal**: Operators can see each energy packet's fill progress, deadline countdown, and budget remaining as visual cards grouped by status.

**Independent Test**: With a live VEN that has an active user request, navigate to `/planner` → Packet Board shows EV packet in "Active" group with fill gauge, "T−Xh Xm" countdown, and status chip. Confirm `npm test` passes PacketProgressBoard unit tests.

### Tests for User Story 2 ⚠️ Write FIRST, confirm FAIL before implementation

- [x] T023 [P] [US2] Write vitest unit tests in `VEN/ui/src/__tests__/PacketProgressBoard.test.tsx` covering:
  - Empty state when `packets` is `[]` (renders `data-testid="packet-board-empty"`)
  - ACTIVE packet appears in Active group (`data-testid="packet-group-active"` contains card)
  - SCHEDULED/PENDING packets appear in Queued group (`data-testid="packet-group-queued"`)
  - COMPLETED/ABANDONED/FAILED appear in Done group (`data-testid="packet-group-done"`) — collapsed by default
  - Fill gauge color: >80% → `success`, 40–80% → `warning`, <40% → `error` (check MUI color prop on `data-testid="packet-fill-{id}"`)
  - Future deadline shows "T−Xh Xm" text in `data-testid="packet-deadline-{id}"`
  - Past deadline shows chip with label "OVERDUE" in `data-testid="packet-deadline-{id}"`
  - No deadline tiers → deadline field absent
  - Budget bar present when `max_total_cost_eur` is set; absent when null
  - Expand card button shows tiers table (`data-testid="packet-tiers-{id}"`)
  - ABANDONED card shows final fill % and active tier index at termination

### Implementation for User Story 2

- [x] T024 [US2] Implement packet status grouping logic in `VEN/ui/src/components/planner/PacketProgressBoard.tsx`: partition `packets` into Active (ACTIVE), Queued (PENDING, SCHEDULED, PAUSED), Done (COMPLETED, PARTIAL_COMPLETED, ABANDONED, FAILED)
- [x] T025 [US2] Implement collapsible group sections in `PacketProgressBoard.tsx`: MUI `Accordion` or `Collapse` for each group; Active and Queued expanded by default; Done collapsed; `data-testid="packet-group-active"`, `packet-group-queued"`, `packet-group-done"`
- [x] T026 [US2] Implement packet card layout in `PacketProgressBoard.tsx`: MUI `Card` per packet, `data-testid="packet-card-{packet.id}"`; header row with asset label, last-6-chars packet ID, and status chip (color: ACTIVE→success, SCHEDULED→info, PENDING→default, PAUSED→warning, COMPLETED→success, ABANDONED→error, FAILED→error)
- [x] T027 [US2] Implement fill gauge in `PacketProgressBoard.tsx`: MUI `LinearProgress` with `value={estimated_completion * 100}`, color = success if >0.8, warning if 0.4–0.8, error if <0.4; `data-testid="packet-fill-{packet.id}"`; label showing percentage
- [x] T028 [US2] Implement deadline countdown logic in `PacketProgressBoard.tsx`: read `deadline_tiers[active_tier_index].deadline`; if absent omit field; if future compute "T−Xh Xm" (hours + minutes until deadline); if past render MUI `Chip color="error" label="OVERDUE"`; `data-testid="packet-deadline-{packet.id}"`
- [x] T029 [US2] Implement budget bar in `PacketProgressBoard.tsx`: only render when `deadline_tiers[active_tier_index].max_total_cost_eur` is not null; MUI `LinearProgress` with `value=min(accumulated_cost_eur/max_total_cost_eur*100, 100)`, color error if >90%; label showing `€X.XX / €Y.YY`; `data-testid="packet-budget-{packet.id}"`
- [x] T030 [US2] Implement card expand/collapse in `PacketProgressBoard.tsx`: expand button `data-testid="packet-expand-{packet.id}"` toggles `data-testid="packet-tiers-{packet.id}"` table showing: tier index, deadline (formatted), min_completion %, max_cost (or "—"); ABANDONED/FAILED cards additionally show active_tier_index and final fill %
- [x] T031 [US2] Implement empty state in `PacketProgressBoard.tsx`: when `packets.length === 0`, render `data-testid="packet-board-empty"` with "No energy packets" text
- [x] T032 [US2] Implement BDD step definitions in `tests/features/steps/planner_ui_steps.py` for Packet Board scenarios: assert-board-visible, assert-active-packet-in-group, assert-fill-gauge, click-expand-card, assert-tiers-table, assert-overdue
- [x] T033 [US2] Verify all Packet Board vitest tests pass (`npm test -- PacketProgressBoard`) and BDD Packet Board scenarios pass on Pi4

**Checkpoint**: US2 is independently functional. Packet Board shows live packet cards with fill and deadline.

---

## Phase 5: User Story 3 — Trigger Timeline (Priority: P3)

**Goal**: Engineers can see a chronological strip of controller events and trace cause-and-effect chains (e.g., RateChange → PlanCycle).

**Independent Test**: Navigate to `/planner` → Trigger Timeline shows at least one chip → click chip → popover appears with event detail. Confirm `npm test` passes PlanTriggerTimeline unit tests.

### Tests for User Story 3 ⚠️ Write FIRST, confirm FAIL before implementation

- [x] T034 [P] [US3] Write vitest unit tests in `VEN/ui/src/__tests__/PlanTriggerTimeline.test.tsx` covering:
  - Empty state when `events` is `[]` (renders appropriate empty text in `data-testid="trigger-timeline"`)
  - Renders correct number of chips (`data-testid="trigger-chip-{i}"` for each event)
  - PlanCycle chip has correct color derived from `trigger_reason`
  - RateChange chip shows import tariff as label text
  - Click chip → popover visible (`data-testid="trigger-popover"`)
  - Click away / Escape → popover closes
  - Chips ordered oldest-left, newest-right (correct order from input array which is newest-first)

### Implementation for User Story 3

- [x] T035 [US3] Implement horizontal scrollable container in `VEN/ui/src/components/planner/PlanTriggerTimeline.tsx`: MUI `Box` with `overflow-x: auto`, `display: flex`, `gap: 1`, `p: 1`; `data-testid="trigger-timeline"`; auto-scroll to right end on mount via `useRef` + `scrollLeft = scrollWidth`
- [x] T036 [US3] Implement event chip rendering in `PlanTriggerTimeline.tsx`: reverse the `events` array (input is newest-first, render oldest-left); for each event render a chip (`data-testid="trigger-chip-{i}"`) with shape/color/label per type:
  - `PlanCycle`: filled circle chip, color by `trigger_reason` (same map as Plan Header trigger badge), label "Plan"
  - `RateChange`: outlined chip, color "info", label = `import_eur_kwh.toFixed(3) €`
  - `CapacityChange`: outlined chip, color "warning", label = `import_limit_kw != null ? ${import_limit_kw}kW : "Cap"`
  - `OpenAdrArrived`: chip, color "success", label = event_name truncated to 12 chars
  - `OpenAdrExpired`: chip, color "default", label = event_name truncated to 12 chars + " ✗"
  - `PacketTransition` / `RequestTransition`: chip, color by `to_status`, label = `${asset_id}: ${from_status}→${to_status}`
- [x] T037 [US3] Implement click popover in `PlanTriggerTimeline.tsx`: MUI `Popover` anchored to clicked chip, `data-testid="trigger-popover"`; content shows all fields of the clicked `TraceEntry` formatted as key:value lines; close on backdrop click
- [x] T038 [US3] Implement empty state in `PlanTriggerTimeline.tsx`: when `events.length === 0`, show MUI `Typography` "No controller events recorded yet" inside the timeline container
- [x] T039 [US3] Implement BDD step definitions in `tests/features/steps/planner_ui_steps.py` for Trigger Timeline scenarios: assert-timeline-visible, assert-chip-count-gte, click-first-chip, assert-popover-visible
- [x] T040 [US3] Verify all Trigger Timeline vitest tests pass (`npm test -- PlanTriggerTimeline`) and BDD Trigger Timeline scenarios pass on Pi4

**Checkpoint**: US3 is independently functional. Trigger Timeline shows event chips with working popover.

---

## Phase 6: User Story 4 — Plan Header (Priority: P4)

**Goal**: Engineers get an immediate at-a-glance status of the current plan: when it ran, what triggered it, committed cost, and any warnings.

**Independent Test**: Navigate to `/planner` → Plan Header shows trigger badge, age, cost, kWh, CO₂. If warnings exist: badge shows count; expand reveals list. Confirm `npm test` passes PlanHeaderBar unit tests.

### Tests for User Story 4 ⚠️ Write FIRST, confirm FAIL before implementation

- [x] T041 [P] [US4] Write vitest unit tests in `VEN/ui/src/__tests__/PlanHeaderBar.test.tsx` covering:
  - Empty state when `plan` is null (renders `data-testid="plan-no-plan"` with "No plan available")
  - Trigger badge shows correct color for each type: Periodic→grey, RateChange→primary, CapacityChange→warning, UserRequest→secondary, Event→success
  - Age text appears in `data-testid="plan-age"` and shows relative format (e.g. "< 1m ago")
  - `data-testid="plan-cost"` contains formatted `firm_summary.total_cost_eur` value
  - `data-testid="plan-import-kwh"` contains formatted `firm_summary.total_import_kwh` value
  - `data-testid="plan-co2"` contains formatted `firm_summary.total_co2_g` value
  - No warnings → `data-testid="plan-warnings-badge"` is absent
  - With 2 warnings → `plan-warnings-badge` shows "2"; click `plan-warnings-expand` shows warnings list
  - Warning list shows severity chip and message text for each warning
  - Re-click expand → list collapses

### Implementation for User Story 4

- [x] T042 [US4] Implement trigger badge in `VEN/ui/src/components/planner/PlanHeaderBar.tsx`: MUI `Chip` with `data-testid="plan-trigger-badge"`, label = `plan.trigger`, color from trigger-type map: `{ Periodic: "default", RateChange: "primary", CapacityChange: "warning", UserRequest: "secondary", Event: "success" }`, fallback color "default"
- [x] T043 [US4] Implement plan age text in `PlanHeaderBar.tsx`: `data-testid="plan-age"`; compute seconds since `plan.created_at`; format as "Xs ago" / "Xm ago" / "Xh ago"; update every render (hook with `Date.now()` via `useMemo` on `dataUpdatedAt` from parent or just compute inline)
- [x] T044 [US4] Implement summary metrics row in `PlanHeaderBar.tsx`: `data-testid="plan-cost"`, `"plan-import-kwh"`, `"plan-co2"` — display `firm_summary.total_cost_eur.toFixed(2) €`, `total_import_kwh.toFixed(1) kWh`, `(total_co2_g / 1000).toFixed(2) kg`
- [x] T045 [US4] Implement warning badge and expand/collapse in `PlanHeaderBar.tsx`: if `plan.warnings.length > 0`: render `data-testid="plan-warnings-badge"` MUI `Badge` showing count; `data-testid="plan-warnings-expand"` button toggles visibility of warnings list; each warning row `data-testid="plan-warning-{i}"` shows severity `Chip` + `message` text + `suggested_action` (if not null)
- [x] T046 [US4] Implement empty state in `PlanHeaderBar.tsx`: when `plan` is null/undefined, render `data-testid="plan-no-plan"` with "No plan available" message
- [x] T047 [US4] Implement BDD step definitions in `tests/features/steps/planner_ui_steps.py` for Plan Header scenarios: assert-header-visible, assert-trigger-badge, assert-plan-age, assert-plan-cost, assert-no-plan-state, click-warnings-expand, assert-warnings-list
- [x] T048 [US4] Verify all Plan Header vitest tests pass (`npm test -- PlanHeaderBar`) and BDD Plan Header scenarios pass on Pi4

**Checkpoint**: US4 is independently functional. Plan Header shows live plan status at a glance.

---

## Phase 7: Page Assembly & End-to-End Wiring

**Purpose**: Wire all four components into `PlannerPage`, finalize routing, ensure the complete page works end-to-end.

### Tests for Page Assembly ⚠️ Write FIRST

- [x] T049 [P] Write vitest unit tests in `VEN/ui/src/__tests__/PlannerPage.test.tsx` covering:
  - Page renders heading `data-testid="planner-heading"`
  - All four section roots present: `plan-header`, `trigger-timeline`, `decision-matrix`, `packet-board`
  - Each section shows empty state when hooks return null/empty (mock all four hooks)
  - Each section shows content when hooks return mock data (one test per section)
- [x] T050 [P] Update `VEN/ui/src/__tests__/App.test.tsx`: add `usePlan`, `useTrace`, `usePackets` mocks if not already present; assert `nav-planner` button exists in rendered navigation

### Implementation for Page Assembly

- [x] T051 Assemble `VEN/ui/src/pages/Planner.tsx`: replace stub with real implementation — `usePlan()`, `useTrace(20)`, `usePackets()` hooks; render `<PlanHeaderBar plan={plan} />`, `<PlanTriggerTimeline events={events ?? []} />`, `<PlanDecisionMatrix plan={plan} />`, `<PacketProgressBoard packets={packets ?? []} />`; section headings (optional) with `data-testid="planner-heading"` on page root
- [x] T052 Run `npm run build` — confirm TypeScript compiles without errors across all new files
- [x] T053 Run `npm test` — confirm all vitest tests pass (existing + all new: PlanHeaderBar, PlanDecisionMatrix, PacketProgressBoard, PlanTriggerTimeline, PlannerPage, App)

**Checkpoint**: Full page works locally. All vitest tests pass. TypeScript clean.

---

## Phase 8: BDD Integration (Green)

**Purpose**: Implement all remaining BDD step definitions and confirm every scenario passes on Pi4.

- [x] T054 Rebuild `test-ven-ui` Docker image on Pi4 (VEN UI source changed): `docker compose -f tests/docker-compose.test.yml build test-ven-ui`
- [x] T055 Complete any remaining stub steps in `tests/features/steps/planner_ui_steps.py` — all step stubs from T007 must now have real Playwright implementations using `data-testid` selectors via `tid()` helper
- [x] T056 Run full `ven_ui_planner.feature` BDD suite on Pi4 — all scenarios must pass: `docker compose -f tests/docker-compose.test.yml run --build --rm test-runner features/ven_ui_planner.feature`
- [x] T057 Run full `@ven-ui` tagged BDD suite on Pi4 — confirm no regressions in existing ven_ui_raw_diagnostics scenarios: `docker compose -f tests/docker-compose.test.yml run --build --rm test-runner --tags @ven-ui`

**Checkpoint**: All BDD scenarios green. No regressions in existing tests.

---

## Phase 9: Polish & Cross-Cutting Concerns

- [x] T058 [P] Manual smoke test on Pi4 browser: navigate to `/planner`, verify all four sections render with live data, click matrix cells, expand packet cards, interact with trigger timeline popover
- [x] T059 Update `docs/history/project_journal.md` with implementation notes: what was built, key decisions made (Decision Matrix as CSS grid, BDD-first approach, type extensions), any issues encountered

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Phase 1 — **BLOCKS all user story work**
- **US1 Decision Matrix (Phase 3)**: Depends on Foundation — can start first
- **US2 Packet Board (Phase 4)**: Depends on Foundation — can run in parallel with US1
- **US3 Trigger Timeline (Phase 5)**: Depends on Foundation — can run in parallel with US1/US2
- **US4 Plan Header (Phase 6)**: Depends on Foundation — can run in parallel with US1/US2/US3
- **Page Assembly (Phase 7)**: Depends on all four user stories complete
- **BDD Green (Phase 8)**: Depends on Page Assembly
- **Polish (Phase 9)**: Depends on BDD Green

### User Story Dependencies

- **US1 (P1)**: No dependency on US2/US3/US4 — fully independent component and tests
- **US2 (P2)**: No dependency on US1/US3/US4 — fully independent component and tests
- **US3 (P3)**: No dependency on US1/US2/US4 — fully independent component and tests
- **US4 (P4)**: No dependency on US1/US2/US3 — fully independent component and tests
- All four stories use different data sources (`plan.steps`, `packets`, `trace/events`, `plan.firm_summary`) — no shared state between components

### Within Each User Story

1. Write vitest tests **FIRST** (T011/T023/T034/T041) — confirm they FAIL
2. Implement component tasks in order (each builds on the previous)
3. Implement BDD steps for that story
4. Verify both vitest and BDD pass before moving on

### Parallel Opportunities

Within Foundation:
- T003, T004, T005 (type additions) can run in parallel [different sections of types.ts]
- T006, T007 (BDD files) can run in parallel [different files]
- T009, T010 (route + page stub) can run in parallel

Across User Stories (after Foundation complete):
- T011 (US1 tests), T023 (US2 tests), T034 (US3 tests), T041 (US4 tests) — all write-tests-first tasks can run in parallel
- T012–T022 (US1 impl), T023–T033 (US2 impl), T034–T040 (US3 impl), T041–T048 (US4 impl) — entire user stories in parallel
- T049 (PlannerPage tests), T050 (App.test update) — parallel in Phase 7

---

## Parallel Example: All Four User Stories

```
# After Foundation (T001–T010) complete:

Agent A (US1 - Decision Matrix):
  T011 → T012 → T013 → T014 → T015 → T016 → T017 → T018 → T019 → T020 → T021 → T022

Agent B (US2 - Packet Board):
  T023 → T024 → T025 → T026 → T027 → T028 → T029 → T030 → T031 → T032 → T033

Agent C (US3 - Trigger Timeline):
  T034 → T035 → T036 → T037 → T038 → T039 → T040

Agent D (US4 - Plan Header):
  T041 → T042 → T043 → T044 → T045 → T046 → T047 → T048

# All agents converge at Phase 7: Page Assembly
```

---

## Implementation Strategy

### MVP First (US1 Only — Decision Matrix)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundation (types + BDD scaffold + route stub)
3. Complete Phase 3: US1 — Decision Matrix
4. **STOP and VALIDATE**: Navigate to `/planner`, confirm Decision Matrix renders and cell-click drawer works. Run `npm test`.
5. Deploy to Pi4 if confirmed working

### Incremental Delivery

1. Foundation → Route stub (empty page exists) → Deploy
2. + US1 (Decision Matrix) → Test independently → Deploy
3. + US2 (Packet Board) → Test independently → Deploy
4. + US3 (Trigger Timeline) → Test independently → Deploy
5. + US4 (Plan Header) → Test independently → Deploy
6. Page Assembly + BDD Green → Full page complete → Deploy

### Full Parallel Strategy

1. One agent completes Foundation (T001–T010)
2. Then:
   - US1 agent: T011–T022
   - US2 agent: T023–T033
   - US3 agent: T034–T040
   - US4 agent: T041–T048
3. Page Assembly: T049–T053
4. BDD Green: T054–T057
5. Polish: T058–T059

---

## Notes

- `[P]` tasks have no dependencies on other incomplete tasks in their phase — safe to run in parallel
- `[US?]` label maps each task to the user story it delivers for traceability
- **BDD-first is mandatory** (Constitution II): T006/T007/T008 must run before any component code
- **Test-first within each story**: Write vitest tests (T011/T023/T034/T041) before implementation tasks
- After T052 `npm run build`, there must be zero TypeScript errors — `PlanReason` discriminated union requires exhaustive branching in the drawer component
- Always rebuild `test-ven-ui` Docker image before running BDD (T054) — source changes are not auto-detected
- After BDD green: commit with message format matching project history (no co-author footers)
