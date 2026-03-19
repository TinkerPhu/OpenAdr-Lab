# Implementation Plan: VEN Raw Data Diagnostics Page

**Branch**: `006-ven-raw-diagnostics` | **Date**: 2026-03-18 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/006-ven-raw-diagnostics/spec.md`

## Summary

Add a "Raw Data" page to the VEN UI that displays three stacked diagnostic cells — Simulator State, Tariffs, and Timeline — each containing a line chart and a manual refresh button. Each cell independently fetches its data from the corresponding VEN API endpoint on demand. The Timeline cell adds a dropdown to switch between asset/grid series. No new backend endpoints or Rust changes are required; this is purely a React UI addition.

## Technical Context

**Language/Version**: TypeScript 5 (React 18)
**Primary Dependencies**: React 18 + MUI v5 + TanStack React Query v5 + recharts
**Storage**: N/A (read-only diagnostic view; no persistence)
**Testing**: Vitest + @testing-library/react (unit); Python behave BDD (integration); Playwright (E2E)
**Target Platform**: Modern browser; served by nginx on Pi4-Server (ARM64 Docker)
**Project Type**: Web application — single-page React component addition
**Performance Goals**: Chart renders within 500ms of data fetch completing (recharts default)
**Constraints**: Must follow `docs/guidelines/REACT_GUIDELINES.md` — named function components, `data-testid` on all interactive elements, TanStack React Query for all API access
**Scale/Scope**: 3 chart components + 1 page + 1 wrapper component + 1 feature file; ~250–350 lines of new TSX

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-checked after Phase 1 design.*

| Principle | Status | Notes |
|-----------|--------|-------|
| I. OpenADR Spec Fidelity | ✅ PASS | No field renaming — upstream field names (`net_power_w`, `interval_start`, `power_kw`, `ts`) passed directly through to chart `dataKey` props |
| II. BDD-First Testing | ✅ PASS | `ven_ui_raw_diagnostics.feature` MUST be written before implementation starts; acceptance scenarios from spec map directly to BDD scenarios |
| III. Upstream Compatibility | ✅ N/A | No changes to `openleadr-rs` submodule |
| IV. Lean Architecture | ✅ PASS | No new abstractions beyond what's needed; one shared `DiagnosticCell` wrapper (justified: identical loading/error/refresh pattern × 4 cells); no new VenApi methods |
| V. Infrastructure Parity | ✅ PASS | Deploy via standard git push + docker compose build ven-ui on Pi4-Server |

**Post-design re-check**: All gates still pass. No violations introduced.

## Project Structure

### Documentation (this feature)

```text
specs/006-ven-raw-diagnostics/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/
│   └── ui-components.md # Phase 1 output
└── tasks.md             # Phase 2 output (/speckit.tasks — NOT created here)
```

### Source Code (repository root)

```text
VEN/ui/src/
├── pages/
│   └── RawDiagnostics.tsx               [NEW] page with 3 stacked cells
├── components/
│   └── raw-diagnostics/
│       ├── DiagnosticCell.tsx            [NEW] shared cell wrapper (title, refresh button, loading/error)
│       ├── SimProfileChart.tsx           [NEW] categorical line chart for /sim snapshot
│       ├── TariffsLineChart.tsx          [NEW] multi-line time-series for /tariffs intervals
│       ├── TimelineSeriesChart.tsx       [NEW] time-series line with series dropdown for /timeline/all
│       └── colors.ts                     [NEW] shared CHART_COLORS constant
└── __tests__/
    ├── RawDiagnostics.test.tsx           [NEW] page-level unit tests
    └── DiagnosticCell.test.tsx           [NEW] cell wrapper unit tests

VEN/ui/src/App.tsx                        [MODIFY] add route + nav button

tests/
├── features/
│   └── ven_ui_raw_diagnostics.feature   [NEW] BDD acceptance scenarios
└── steps/
    └── ven_ui_raw_diagnostics_steps.py  [NEW] BDD step definitions
```

**Structure Decision**: Web application layout (Option 2 variant). All new code is in `VEN/ui/src/` under `pages/` and `components/raw-diagnostics/`. BDD tests follow the existing tests/ pattern.

## Complexity Tracking

No constitution violations. Table not required.

---

## Phase 0: Research Output

See [research.md](research.md) for all decisions.

**Key resolved unknowns**:
1. `/sim` is a snapshot (not time-series) → categorical x-axis profile line
2. `/tariffs` is currently registered in VenApi as `api.rates()` → use that method
3. `/timeline/all` accepts `hours_back`/`hours_forward` query params (default 1.0 each)
4. Manual fetch pattern: `useQuery({ enabled: false })` + `refetch()` on button click
5. Timeline dropdown: populated from response keys at fetch time; default = "grid"
6. Plan cell dropped from scope — `/plan` endpoint not yet wired into the UI data layer

---

## Phase 1: Design Output

### Data Model

See [data-model.md](data-model.md) for full TypeScript shapes.

**Summary**:
| Cell | Endpoint | Chart Type | X-axis | Y-axis |
|------|----------|-----------|--------|--------|
| Simulator State | `/sim` | Line (categorical) | Asset ID | power_kw |
| Tariffs | `/rates` | Line (time-series) | interval_start (ms) | price (€/kWh) + CO₂ |
| Timeline | `/timeline/all` | Line (time-series) | ts (ms) | power_kw |

### Component Contracts

See [contracts/ui-components.md](contracts/ui-components.md) for full component interfaces and `data-testid` requirements.

**Component tree**:
```
RawDiagnosticsPage
├── DiagnosticCell "Simulator State"
│   └── SimProfileChart
├── DiagnosticCell "Tariffs"
│   └── TariffsLineChart
└── DiagnosticCell "Timeline"
    └── TimelineSeriesChart (+ series dropdown inside)
```

### BDD Scenarios (pre-implementation — must be written first)

Feature file: `tests/features/ven_ui_raw_diagnostics.feature`

```gherkin
Feature: VEN Raw Data Diagnostics Page

  Background:
    Given the VEN UI is open
    And the user navigates to the Raw Data page

  Scenario: Page renders three diagnostic cells
    Then I see the "Simulator State" cell
    And I see the "Tariffs" cell
    And I see the "Timeline" cell

  Scenario: Sim cell refreshes on button click
    When I click the refresh button in the "Simulator State" cell
    Then the Simulator State chart is displayed

  Scenario: Tariffs cell refreshes on button click
    When I click the refresh button in the "Tariffs" cell
    Then the Tariffs chart is displayed

  Scenario: Timeline cell shows series dropdown and refreshes
    When I select "grid" from the Timeline series dropdown
    And I click the refresh button in the "Timeline" cell
    Then the Timeline chart is displayed

  Scenario: Each cell refreshes independently
    When I click the refresh button in the "Simulator State" cell
    Then only the Simulator State cell shows a loading state
    And the other cells remain unchanged

  Scenario: Timeline dropdown filters series
    When I click the refresh button in the "Timeline" cell
    Then the series dropdown lists the available asset series
    When I select "ev" from the Timeline series dropdown
    And I click the refresh button in the "Timeline" cell
    Then the Timeline chart displays data for "ev"
```

### Implementation Order

1. **Write BDD feature file** (red — tests must fail first)
2. **Add route + nav button** in `App.tsx`
3. **Create `colors.ts`** shared constant
4. **Create `DiagnosticCell.tsx`** — wrapper (loading/error/refresh)
5. **Create `SimProfileChart.tsx`**
6. **Create `TariffsLineChart.tsx`**
7. **Create `TimelineSeriesChart.tsx`** (includes dropdown)
8. **Create `RawDiagnostics.tsx`** page — wire all cells together
9. **Write vitest unit tests** for `DiagnosticCell` and `RawDiagnostics`
10. **Write BDD step definitions** (`ven_ui_raw_diagnostics_steps.py`)
11. **Deploy to Pi4** and run BDD tests green
