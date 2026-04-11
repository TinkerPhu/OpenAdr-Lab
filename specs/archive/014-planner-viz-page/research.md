# Research: Planner Visualization Page

**Branch**: `014-planner-viz-page` | **Date**: 2026-04-04

## Decision 1: `GET /plan` Fetch Mode

**Question**: Does `api.plan()` already fetch the full plan (with `steps[]`) or only the summary?

**Finding**: `VEN/ui/src/api/client.ts:155` — `async plan()` calls `GET /plan` (no `?summary` suffix). The full plan including `steps[]` is already fetched. However, the TypeScript `Plan` type in `types.ts:210` does NOT include `steps`, `packets`, or `envelopes` fields — these were simply not typed when the endpoint was first wired up.

**Decision**: No change to `api.plan()` or the backend. Extend the `Plan` TypeScript type and add `PlanStep` / `PlanReason` types to `types.ts`.

**Rationale**: Zero-cost path — backend already returns steps. Typing the existing response is pure TypeScript work.

---

## Decision 2: TypeScript Types to Add

**Finding**: Current `types.ts` is missing:
- `PlanStep` struct (one per asset per slot in the audit trail)
- `PlanReason` discriminated union (12 variants)
- `steps: PlanStep[]` field on `Plan`

These map directly to the Rust structs in `VEN/src/entities/plan.rs`. The 12 `PlanReason` variants with their numeric parameters are confirmed from the backend exploration.

**Decision**: Add `PlanStep`, `PlanReason`, and extend `Plan` in `types.ts`. Keep field names exactly as the backend serializes (snake_case, no renaming per Constitution Principle I).

---

## Decision 3: Decision Matrix Rendering Technology

**Question**: Should the Decision Matrix use a charting library (recharts) or plain CSS/MUI grid?

**Finding**: The matrix is a 2D array of small colored boxes — not a time-series chart. Recharts is designed for line/bar/area charts; forcing it here adds complexity. MUI `Box` components with CSS grid layout (`display: grid; grid-template-columns: repeat(N, ...)`) gives full control at zero extra dependency cost. The ControllerV2 asset timeline uses recharts for power-over-time charts — that's appropriate. The Decision Matrix is categorically different.

**Decision**: Implement Decision Matrix as a CSS grid of MUI `Box` cells with `overflow-x: scroll`. No recharts, no new dependencies.

**Rationale**: Lean Architecture (Constitution IV) — the simplest solution that meets the requirement. Recharts would add ~30KB and configuration complexity with no benefit for a grid of colored boxes.

**Alternatives considered**:
- recharts heatmap plugin — not a built-in chart type; would require a third-party plugin
- @mui/x-data-grid — over-engineered; adds a heavy dependency for a fixed-column layout

---

## Decision 4: Trigger Timeline vs Existing Trace Page

**Question**: Should the Trigger Timeline replace the existing `/trace` Trace page, or coexist?

**Finding**: The existing `TracePage` (`VEN/ui/src/pages/Trace.tsx`) renders a full-page scrollable table of all controller events. The new Trigger Timeline is a compact horizontal strip showing ~20 recent events as chips, embedded inside the Planner page. They serve different purposes: the Trace page gives complete raw history; the Trigger Timeline gives at-a-glance causation context for the current plan.

**Decision**: Coexist. The Trigger Timeline is a new embedded component on the Planner page. The existing Trace page remains unchanged. Both consume `useTrace()` → the Planner page passes a lower `limit` (20 events) for compact display.

**Rationale**: Removing the Trace page would break existing BDD navigation tests. The Trigger Timeline component reuses all existing data-fetching logic.

---

## Decision 5: BDD Test Coverage Approach

**Question**: What test files are needed and what's the right pattern?

**Finding**:
- BDD feature files at `tests/features/`, tagged `@ven-ui` for Playwright-driven UI tests
- Step definitions in `tests/features/steps/controller_ui_steps.py`, `controller_steps.py`, etc.
- UI helper uses `data-testid` selectors via `tid()` function (`helpers/ui.py`)
- Vitest unit tests at `VEN/ui/src/__tests__/` — each page/component has a corresponding `.test.tsx`
- Pattern: mock all hooks with `vi.mock("../api/hooks", ...)` at test module level

**Decision**:
1. New BDD feature: `tests/features/ven_ui_planner.feature` — covers navigation, section rendering, empty states, and key interactions (cell click → drawer, expand/collapse, packet card groups)
2. New step file: `tests/features/steps/planner_ui_steps.py` — Playwright steps using `data-testid`
3. New vitest unit tests: `VEN/ui/src/__tests__/PlannerPage.test.tsx` (page-level) + individual component tests for `PlanHeaderBar`, `PlanDecisionMatrix`, `PacketProgressBoard`, `PlanTriggerTimeline`

**Rationale**: BDD covers user-observable behavior (Constitution II). Vitest covers component rendering logic, data formatting, color coding, and edge cases that are too fine-grained for BDD.

---

## Decision 6: Plan Header Placement vs Existing Controller Page

**Question**: Does the new Plan Header duplicate information already on the Controller page?

**Finding**: The Controller page (`Controller.tsx`) has a `PlanCard` that shows trigger, cost, import kWh, and warning count as a small card in a 3-column status bar. The new Planner page Plan Header shows the same data but as a prominent full-width bar with expandable warnings.

**Decision**: No conflict — different levels of detail for different pages. The Controller page is a quick-glance status card; the Planner page is a deep-dive diagnostic tool. No code sharing is forced (the Controller's `PlanCard` is small enough that abstracting it would be premature).

**Rationale**: Lean Architecture — three similar lines are better than a premature abstraction.

---

## Decision 7: FIRM-Only Default View Column Count

**Question**: How many FIRM slots should be shown by default?

**Finding**: Backend profile default is `step_size_s: 300` (5 minutes) with near-horizon at 3 hours. That gives 36 FIRM slots. At a cell width of ~24px, 36 slots = ~864px, fitting a 1280px-wide monitor with the asset label column on the left. Full horizon = 288 slots = ~6,912px requiring horizontal scroll.

**Decision**: Default view shows all FIRM-zone columns (~36). The "Expand" button reveals all columns including FLEXIBLE. A collapse-entire-section button also exists for when the matrix is not needed.

---

## Decision 8: `suggested_action` Field on Plan Warnings

**Question**: The `Plan.warnings` array in `types.ts` lacks `suggested_action`. Does the backend actually return it?

**Finding**: Backend `PlanWarning` struct in `VEN/src/entities/plan.rs` has `suggested_action: Option<String>`. Currently the TypeScript type is `warnings: Array<{ severity: string; message: string; packet_id: string | null }>` — missing `suggested_action`.

**Decision**: Add `suggested_action: string | null` to the warnings array type in `types.ts`.
