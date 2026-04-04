# Implementation Plan: Planner Visualization Page

**Branch**: `014-planner-viz-page` | **Date**: 2026-04-04 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/014-planner-viz-page/spec.md`

## Summary

Add a dedicated **Planner** page to the VEN Web UI that makes the HEMS planner's decisions fully
transparent. The page contains four stacked sections: a plan summary header (trigger, cost, warnings),
a horizontal trigger timeline (controller event history as chips), a decision matrix (time × asset
heatmap with per-slot planning reasons), and a packet progress board (energy packet cards with fill
gauges and deadline countdowns).

All data is available via existing backend endpoints (`GET /plan`, `GET /trace/events`,
`GET /packets`). The only code changes needed are: TypeScript type additions in `types.ts`
(add `PlanStep`, `PlanReason`, extend `Plan`), four new React components, one new page,
a route/tab registration in `App.tsx`, vitest unit tests for each component, and one new BDD feature
file with Playwright steps.

## Technical Context

**Language/Version**: TypeScript 5 (React 18)
**Primary Dependencies**: MUI v5, TanStack React Query v5, React Router v6 (all existing)
**Storage**: N/A — read-only diagnostic view; no persistence
**Testing**: Vitest + @testing-library/react (unit); Python behave + Playwright (BDD)
**Target Platform**: Desktop browser (Linux ARM64 Pi4 via Docker nginx)
**Project Type**: Web application — frontend only (no backend changes)
**Performance Goals**: Page loads with meaningful content in < 3s on Pi4; default view renders ≤ 36 matrix columns to stay snappy
**Constraints**: No new npm dependencies; no new backend endpoints; mobile responsiveness out of scope
**Scale/Scope**: ≤ 5 assets per VEN; 36 FIRM slots + 252 FLEXIBLE slots per plan (5-min steps, 24h horizon)

## Constitution Check

### I. OpenADR Spec Fidelity ✅
All field names used match the backend serialization verbatim: `firm_boundary`, `steps`, `asset_id`,
`setpoint_kw`, `actual_power_kw`, `import_tariff_eur_kwh`, `estimated_completion`, `accumulated_cost_eur`,
`deadline_tiers`, `active_tier_index`, `trigger_reason`, etc. No renaming at any layer.

### II. BDD-First Testing ✅
New `tests/features/ven_ui_planner.feature` must be written first, confirmed red, then implementation.
All acceptance scenarios from spec.md map to BDD scenarios. Vitest unit tests supplement BDD for
component-level logic (cell colors, OVERDUE logic, grouping).

### III. Upstream Compatibility ✅
This feature is 100% inside `VEN/ui/` (our code) — no changes to the `openleadr-rs` submodule.
Not applicable to upstream PR workflow.

### IV. Lean Architecture ✅
No new npm dependencies. No new backend endpoints. Decision Matrix uses CSS grid + MUI Box cells
(not a charting library). The Trigger Timeline reuses `useTrace()` and `TraceEntry` types unchanged.
No shared abstractions created between `PlanHeaderBar` and the existing `Controller.tsx` PlanCard
(different detail levels, premature to abstract).

### V. Infrastructure Parity ✅
Test runs on Pi4-Server via SSH. Docker Compose test definitions unchanged. `test-ven-ui` image
must be rebuilt explicitly when UI source changes — documented in quickstart.md.

**Complexity Tracking**: No violations. No new table entries required.

## Project Structure

### Documentation (this feature)

```text
specs/014-planner-viz-page/
├── plan.md              ← this file
├── research.md          ← Phase 0: all decisions resolved
├── data-model.md        ← Phase 1: new types + data flow
├── quickstart.md        ← Phase 1: dev setup + test commands + data-testid map
├── checklists/
│   └── requirements.md  ← spec quality checklist (all green)
└── tasks.md             ← Phase 2 output (from /speckit.tasks)
```

### Source Code

```text
VEN/ui/
├── src/
│   ├── api/
│   │   └── types.ts                         MODIFY: add PlanStep, PlanReason, extend Plan
│   ├── pages/
│   │   └── Planner.tsx                      CREATE: main page, four sections
│   ├── components/
│   │   └── planner/
│   │       ├── PlanHeaderBar.tsx            CREATE
│   │       ├── PlanTriggerTimeline.tsx      CREATE
│   │       ├── PlanDecisionMatrix.tsx       CREATE
│   │       └── PacketProgressBoard.tsx      CREATE
│   ├── App.tsx                              MODIFY: add /planner route + nav tab
│   └── __tests__/
│       ├── PlannerPage.test.tsx             CREATE
│       ├── PlanHeaderBar.test.tsx           CREATE
│       ├── PlanDecisionMatrix.test.tsx      CREATE
│       ├── PacketProgressBoard.test.tsx     CREATE
│       └── PlanTriggerTimeline.test.tsx     CREATE
└── (no other files modified)

tests/features/
├── ven_ui_planner.feature                   CREATE: BDD scenarios (@ven-ui tag)
└── steps/
    └── planner_ui_steps.py                  CREATE: Playwright step definitions
```

---

## Implementation Phases

### Phase A — Type Extensions (foundation, no UI yet)

**Files**: `VEN/ui/src/api/types.ts`

1. Add `PlanReason` discriminated union (12 variants with parameters)
2. Add `PlanStep` type
3. Extend `Plan` with `firm_boundary: string` and `steps: PlanStep[]`
4. Add `suggested_action: string | null` to `Plan.warnings` array

No component changes. Vitest type-checks pass immediately if types are correct.

**Deliverable**: `types.ts` compiles cleanly; existing tests still pass.

---

### Phase B — BDD Feature File (write first, confirm red)

**Files**: `tests/features/ven_ui_planner.feature`, `tests/features/steps/planner_ui_steps.py`

Write all BDD scenarios before any implementation. Run once to confirm they fail with "step not
implemented" or "element not found". This satisfies Constitution II (BDD-First).

**BDD Scenarios to write**:

```gherkin
@ven-ui
Feature: Planner Visualization Page

  Background:
    Given the VEN UI is open

  # ── Navigation ──────────────────────────────────────────────────────────

  Scenario: Planner tab appears in navigation
    Then I see the "Planner" navigation tab

  Scenario: Navigate to Planner page
    When I click the "Planner" navigation tab
    Then I am on the Planner page

  # ── Plan Header ──────────────────────────────────────────────────────────

  Scenario: Plan header shows summary when plan exists
    When I navigate to the Planner page
    Then the plan header section is visible
    And the trigger badge is displayed
    And the plan age text is displayed
    And the committed cost value is displayed

  Scenario: Plan header shows empty state when no plan
    Given no plan has been generated
    When I navigate to the Planner page
    Then the plan header shows "No plan available"

  Scenario: Plan warnings expand and collapse
    Given the current plan has at least one warning
    When I navigate to the Planner page
    Then the warning count badge is visible
    When I click the expand warnings button
    Then the warnings list is shown with severity and message
    When I click the expand warnings button again
    Then the warnings list is hidden

  # ── Trigger Timeline ─────────────────────────────────────────────────────

  Scenario: Trigger timeline shows controller events
    When I navigate to the Planner page
    Then the trigger timeline section is visible
    And at least one event chip is displayed

  Scenario: Clicking an event chip shows detail popover
    When I navigate to the Planner page
    And I click the first event chip in the trigger timeline
    Then a popover appears with event detail

  # ── Decision Matrix ──────────────────────────────────────────────────────

  Scenario: Decision matrix renders with cells and tariff header
    When I navigate to the Planner page
    Then the decision matrix section is visible
    And the tariff header row is displayed
    And at least one asset row is displayed

  Scenario: Decision matrix shows FIRM/FLEX boundary divider
    When I navigate to the Planner page
    Then the FIRM/FLEX boundary divider is visible in the matrix

  Scenario: Clicking a matrix cell opens step detail drawer
    When I navigate to the Planner page
    And I click the first visible matrix cell
    Then the step detail drawer opens
    And the drawer shows the reason type
    And the drawer shows setpoint and actual power values

  Scenario: Decision matrix collapses and expands
    When I navigate to the Planner page
    And I click the collapse matrix button
    Then the decision matrix cells are hidden
    When I click the expand matrix button
    Then the decision matrix cells are visible

  Scenario: Decision matrix shows empty state when no plan
    Given no plan has been generated
    When I navigate to the Planner page
    Then the decision matrix shows an empty state message

  # ── Packet Board ─────────────────────────────────────────────────────────

  Scenario: Packet board renders with active packet
    Given an active energy packet exists for the EV
    When I navigate to the Planner page
    Then the packet board section is visible
    And the active packet group shows the EV packet
    And the packet fill gauge is displayed

  Scenario: Packet board shows empty state when no packets
    Given no energy packets exist
    When I navigate to the Planner page
    Then the packet board shows an empty state message

  Scenario: Packet card expand shows deadline tiers
    Given an active energy packet exists for the EV
    When I navigate to the Planner page
    And I expand the EV packet card
    Then the deadline tiers table is shown

  Scenario: Overdue packet shows OVERDUE label
    Given an energy packet exists with a past deadline
    When I navigate to the Planner page
    Then the packet card shows "OVERDUE" in red
```

**Deliverable**: Feature file committed; scenarios fail (red). Step stubs in `planner_ui_steps.py`.

---

### Phase C — Plan Header (`PlanHeaderBar`)

**Files**: `VEN/ui/src/components/planner/PlanHeaderBar.tsx`,
`VEN/ui/src/__tests__/PlanHeaderBar.test.tsx`

Component contract:
```typescript
function PlanHeaderBar({ plan }: { plan: Plan | null | undefined })
```

Renders:
- Empty state (`data-testid="plan-no-plan"`) when `plan` is null/undefined
- Trigger badge with color from trigger type map
- Relative age (seconds/minutes since `plan.created_at`)
- `firm_summary.total_cost_eur`, `total_import_kwh`, `total_co2_g`
- Warning count badge → click to expand list of `{ severity, message, suggested_action }`

Vitest coverage:
- Renders empty state when plan is null
- Shows correct trigger badge color for each type (Periodic/RateChange/etc.)
- Shows "3s ago" / "2m ago" relative time formatting
- Warning badge shows count; expand reveals list; re-click collapses
- No warnings → no badge visible

---

### Phase D — Trigger Timeline (`PlanTriggerTimeline`)

**Files**: `VEN/ui/src/components/planner/PlanTriggerTimeline.tsx`,
`VEN/ui/src/__tests__/PlanTriggerTimeline.test.tsx`

Component contract:
```typescript
function PlanTriggerTimeline({ events }: { events: TraceEntry[] })
```

Renders:
- Horizontal scrollable `Box` (`data-testid="trigger-timeline"`)
- Empty state when `events.length === 0`
- One chip per event (`data-testid="trigger-chip-{i}"`)
  - Shape: circle (PlanCycle), diamond (RateChange/CapacityChange), star (OpenAdrArrived/Expired), arrow (PacketTransition/RequestTransition)
  - Color: trigger-type-coded (PlanCycle colored by trigger_reason, others by type)
  - Label: short (e.g. "Plan", tariff value, event name truncated to 12 chars)
- Click chip → popover with formatted event detail (`data-testid="trigger-popover"`)
- Newest chip on right; strip auto-scrolls to right end on mount

Vitest coverage:
- Empty state when no events
- Renders correct number of chips
- PlanCycle chip has correct color for trigger_reason
- RateChange chip shows tariff value as label
- Click chip shows popover; click away closes it
- Chips render in chronological order (oldest left, newest right)

---

### Phase E — Decision Matrix (`PlanDecisionMatrix`)

**Files**: `VEN/ui/src/components/planner/PlanDecisionMatrix.tsx`,
`VEN/ui/src/__tests__/PlanDecisionMatrix.test.tsx`

Component contract:
```typescript
function PlanDecisionMatrix({ plan }: { plan: Plan | null | undefined })
```

This is the most complex component. Internal structure:

1. **Derive matrix data**: Group `plan.steps` by `asset_id`, sort by `ts`. Build `MatrixCell[][]` (rows × cols).
2. **Tariff header**: One `Box` per column, background color interpolated from `PlanTimeSlot.import_tariff_eur_kwh` across [min, max] → green to red.
3. **FIRM/FLEX divider**: Find column index where `ts >= plan.firm_boundary`. Render a `Box` with left border at that index. Mark FLEXIBLE columns at 50% opacity.
4. **Asset rows**: One `Box` per step, colored by `REASON_META[step.reason.type].color`. Each cell has `data-testid="matrix-cell-{assetId}-{slotIndex}"` and a `title` tooltip.
5. **Step detail drawer**: MUI `Drawer` anchored to right. Opens on cell click. Shows:
   - Timestamp (formatted)
   - Asset ID
   - `setpoint_kw` and `actual_power_kw`
   - Reason type + parameters (e.g. for CheapTariff: tariff value and threshold)
   - `state_before`, `avail_max_import_kw`, `avail_max_export_kw`
6. **Collapse section**: `data-testid="matrix-collapse-btn"` hides cell grid but keeps header visible.
7. **Expand horizon**: `data-testid="matrix-expand-horizon-btn"` shows FLEXIBLE columns (default FIRM-only).
8. **Legend**: Static MUI `Table` or `Stack` below the grid showing all 12 reasons with color swatch and label.

Vitest coverage:
- Empty state when plan is null
- Renders correct number of rows (one per unique asset_id in steps)
- Cell has correct background color for reason type
- FIRM cells are full opacity; FLEXIBLE cells have reduced opacity
- FIRM/FLEX divider element exists
- Click cell → drawer opens with reason type text visible
- Collapse button hides cells; expand button shows them
- Expand horizon button adds more columns

---

### Phase F — Packet Board (`PacketProgressBoard`)

**Files**: `VEN/ui/src/components/planner/PacketProgressBoard.tsx`,
`VEN/ui/src/__tests__/PacketProgressBoard.test.tsx`

Component contract:
```typescript
function PacketProgressBoard({ packets }: { packets: EnergyPacket[] })
```

Renders:
- Empty state (`data-testid="packet-board-empty"`) when `packets.length === 0`
- Three collapsible groups: Active, Queued, Done
- Active and Queued expanded by default; Done collapsed by default
- Per card (`data-testid="packet-card-{packet.id}"`):
  - Asset label + last 6 chars of packet ID
  - Status chip (MUI Chip color = ACTIVE→success, SCHEDULED→info, PENDING→default, etc.)
  - Fill gauge: MUI `LinearProgress` (`data-testid="packet-fill-{packet.id}"`), value = `estimated_completion * 100`, color = success (>80%) / warning (40–80%) / error (<40%)
  - Deadline countdown (`data-testid="packet-deadline-{packet.id}"`): compute from `deadline_tiers[active_tier_index].deadline`:
    - If future: "T−2h30m" format
    - If past: `<Chip color="error" label="OVERDUE" />`
    - If no tiers: omit field
  - Budget bar (`data-testid="packet-budget-{packet.id}"`): shown only if `deadline_tiers[active_tier_index].max_total_cost_eur` is not null. MUI LinearProgress value = `accumulated_cost_eur / max_total_cost_eur * 100`, capped display at 100 but can overflow visually with error color.
  - Expand button → shows `deadline_tiers` table (`data-testid="packet-tiers-{packet.id}"`)
  - ABANDONED/FAILED: show `active_tier_index` at termination + final `estimated_completion`

Vitest coverage:
- Empty state when no packets
- Active packet appears in Active group
- SCHEDULED/PENDING appear in Queued group
- COMPLETED/ABANDONED appear in Done group (collapsed by default → not visible until expanded)
- Fill gauge color: >80% → success, 40–80% → warning, <40% → error
- Deadline in future → "T−Xh Xm" text visible
- Deadline in past → "OVERDUE" chip visible
- Budget bar absent when max_total_cost_eur is null
- Budget bar present and correct value when limit set
- Expand card → tiers table appears

---

### Phase G — Page Assembly + Routing

**Files**: `VEN/ui/src/pages/Planner.tsx`, `VEN/ui/src/App.tsx`,
`VEN/ui/src/__tests__/PlannerPage.test.tsx`

`Planner.tsx`:
```typescript
export function PlannerPage() {
  const { data: plan } = usePlan();
  const { data: events } = useTrace(20);
  const { data: packets } = usePackets();
  return (
    <Stack spacing={3}>
      <Typography variant="h5" data-testid="planner-heading">Planner</Typography>
      <PlanHeaderBar plan={plan} />
      <PlanTriggerTimeline events={events ?? []} />
      <PlanDecisionMatrix plan={plan} />
      <PacketProgressBoard packets={packets ?? []} />
    </Stack>
  );
}
```

`App.tsx` changes:
- Import `PlannerPage`
- Add nav button: `<Button component={Link} to="/planner" data-testid="nav-planner">Planner</Button>`
  (position: after "Controller V2", before "User Requests")
- Add route: `<Route path="/planner" element={<PlannerPage />} />`

`PlannerPage.test.tsx`:
- All four sections render when plan/events/packets data provided
- All four sections show empty states when data is null/empty
- Mock: `vi.mock("../api/hooks", () => ({ usePlan, useTrace, usePackets }))`

`App.test.tsx` update:
- Add mock for `usePlan` (already needed by Controller.tsx tests — verify it exists)
- Assert `nav-planner` button exists in rendered App

---

### Phase H — BDD Steps Implementation

**Files**: `tests/features/steps/planner_ui_steps.py`

Implement the Playwright step definitions written in Phase B. Pattern follows `controller_ui_steps.py`:
- Use `tid()` helper for `data-testid` selectors
- Steps for: navigate to Planner, assert section visible, click cell, assert drawer open, etc.
- Where API state is needed (e.g. "given active packet exists"), reuse existing VTN/VEN API helpers from `api_client.py`

---

### Phase I — End-to-End Verification

1. `npm test` — all vitest tests pass
2. `npm run build` — TypeScript compiles without errors
3. SCP `VEN/ui/` to Pi4-Server → rebuild `ven-ui` container
4. Manual navigation: open `/planner`, verify all four sections
5. Run BDD: `docker compose ... run --build test-runner features/ven_ui_planner.feature`
6. Run full @ven-ui suite to confirm no regressions
7. Update `docs/history/project_journal.md`

---

## Risk Notes

- **Decision Matrix performance**: 288 columns × 5 assets = 1440 DOM elements when fully expanded. Default FIRM-only view (36 cols × 5 = 180 elements) is safe. Full expansion on Pi4 browser should be tested. If slow, add virtualization (windowing) only if measured lag > 500ms — not speculatively.
- **`firm_boundary` field absent from old plans**: If the backend was updated to add `firm_boundary` but old cached plans in the UI don't have it, the FIRM/FLEX divider falls back to showing all columns as FIRM. Graceful degradation, no crash.
- **`steps[]` absent from `?summary` calls**: If any other page accidentally calls `GET /plan?summary` via a different code path, steps will be empty. The Decision Matrix will show an empty state. Acceptable; the Planner page always calls full `GET /plan`.
