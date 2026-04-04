# Quickstart: Planner Visualization Page

**Branch**: `014-planner-viz-page` | **Date**: 2026-04-04

## Prerequisites

- VEN stack running on Pi4-Server (ven-1 at port 8211, UI at port 8214)
- At least one plan generated (VEN has been running ≥ 20s after startup)
- Optionally: an active user request to see the Packet Board with a live packet

## Local Development

```bash
cd VEN/ui
npm install          # already done if working in this repo
npm run dev          # starts Vite dev server at http://localhost:5173
```

Navigate to `http://localhost:5173/planner` — the new Planner tab.

To see realistic data, point the VEN selector to a live VEN or mock the hooks in dev.

## Running Tests

### Vitest unit tests (fast, local)

```bash
cd VEN/ui
npm test
```

New test files added by this feature:
- `src/__tests__/PlannerPage.test.tsx` — page layout, section rendering, navigation
- `src/__tests__/PlanHeaderBar.test.tsx` — trigger badge colors, warning expand/collapse
- `src/__tests__/PlanDecisionMatrix.test.tsx` — cell colors, FIRM/FLEX divider, empty state, drawer
- `src/__tests__/PacketProgressBoard.test.tsx` — grouping, fill colors, deadline countdown, OVERDUE
- `src/__tests__/PlanTriggerTimeline.test.tsx` — chip rendering, event type shapes, popover

### BDD integration tests (Pi4, Docker)

```bash
# On Pi4-Server via SSH:
cd /srv/docker/openadr_lab
docker compose -f tests/docker-compose.test.yml run --build --rm test-runner \
  features/ven_ui_planner.feature
```

This builds the test-ven-ui image (must be done explicitly if VEN UI source changed):
```bash
docker compose -f tests/docker-compose.test.yml build test-ven-ui
docker compose -f tests/docker-compose.test.yml run --build --rm test-runner \
  features/ven_ui_planner.feature
```

### Run all @ven-ui BDD tests

```bash
docker compose -f tests/docker-compose.test.yml run --build --rm test-runner \
  --tags @ven-ui
```

## Key `data-testid` Attributes

These are used by both Playwright BDD steps and vitest assertions.

| Component | `data-testid` | Purpose |
|---|---|---|
| App.tsx | `nav-planner` | Navigation button to Planner page |
| PlanHeaderBar | `plan-header` | Section root |
| PlanHeaderBar | `plan-trigger-badge` | Trigger type chip |
| PlanHeaderBar | `plan-age` | Relative timestamp text |
| PlanHeaderBar | `plan-cost` | Firm cost value |
| PlanHeaderBar | `plan-import-kwh` | Import kWh value |
| PlanHeaderBar | `plan-co2` | CO₂ value |
| PlanHeaderBar | `plan-warnings-badge` | Warning count badge |
| PlanHeaderBar | `plan-warnings-expand` | Expand/collapse warnings button |
| PlanHeaderBar | `plan-warning-{i}` | Individual warning row |
| PlanHeaderBar | `plan-no-plan` | Empty state message |
| PlanTriggerTimeline | `trigger-timeline` | Section root |
| PlanTriggerTimeline | `trigger-chip-{i}` | Individual event chip |
| PlanDecisionMatrix | `decision-matrix` | Section root |
| PlanDecisionMatrix | `matrix-collapse-btn` | Collapse/expand section button |
| PlanDecisionMatrix | `matrix-expand-horizon-btn` | Show full horizon button |
| PlanDecisionMatrix | `matrix-cell-{assetId}-{slotIndex}` | Individual cell |
| PlanDecisionMatrix | `matrix-firm-flex-divider` | FIRM/FLEX boundary line |
| PlanDecisionMatrix | `matrix-drawer` | Step detail side-drawer |
| PlanDecisionMatrix | `matrix-drawer-reason` | Reason detail in drawer |
| PlanDecisionMatrix | `matrix-empty` | Empty state |
| PacketProgressBoard | `packet-board` | Section root |
| PacketProgressBoard | `packet-group-active` | Active group container |
| PacketProgressBoard | `packet-group-queued` | Queued group container |
| PacketProgressBoard | `packet-group-done` | Done group container |
| PacketProgressBoard | `packet-card-{packetId}` | Individual packet card |
| PacketProgressBoard | `packet-fill-{packetId}` | Fill gauge bar |
| PacketProgressBoard | `packet-deadline-{packetId}` | Deadline countdown text |
| PacketProgressBoard | `packet-budget-{packetId}` | Budget bar (if shown) |
| PacketProgressBoard | `packet-expand-{packetId}` | Expand/collapse card button |
| PacketProgressBoard | `packet-tiers-{packetId}` | Deadline tiers table |
| PacketProgressBoard | `packet-board-empty` | Empty state |

## File Layout After Implementation

```
VEN/ui/src/
├── api/
│   └── types.ts                         ← add PlanStep, PlanReason, extend Plan
├── pages/
│   └── Planner.tsx                      ← new: main page (4 sections stacked)
├── components/
│   └── planner/
│       ├── PlanHeaderBar.tsx            ← new
│       ├── PlanTriggerTimeline.tsx      ← new
│       ├── PlanDecisionMatrix.tsx       ← new (heaviest)
│       └── PacketProgressBoard.tsx      ← new
└── __tests__/
    ├── PlannerPage.test.tsx             ← new
    ├── PlanHeaderBar.test.tsx           ← new
    ├── PlanDecisionMatrix.test.tsx      ← new
    ├── PacketProgressBoard.test.tsx     ← new
    └── PlanTriggerTimeline.test.tsx     ← new

tests/features/
├── ven_ui_planner.feature               ← new BDD feature
└── steps/
    └── planner_ui_steps.py              ← new BDD step definitions
```

## Verification Checklist

After implementation, verify end-to-end:

1. `npm test` — all vitest tests pass (including new ones)
2. Navigate to `/planner` in browser — Planner tab appears in nav bar
3. Plan Header shows trigger badge + age + cost + kWh + CO₂
4. If warnings exist: badge shows count; expand shows list
5. Trigger Timeline shows chips; click one → popover with event detail
6. Decision Matrix renders colored cells; hover shows reason tooltip
7. Click a cell → drawer opens with step detail and reason parameters
8. FIRM/FLEX divider is visible; flexible cells are visibly faded
9. "Expand horizon" button adds FLEXIBLE columns; collapse button hides section
10. Packet Board shows cards grouped into Active/Queued/Done
11. Fill gauge color reflects completion % (green/amber/red)
12. Deadline shows "T−Xh Xm" or "OVERDUE" in red
13. Expand a card → deadline tiers table appears
14. BDD: `features/ven_ui_planner.feature` — all scenarios pass on Pi4
