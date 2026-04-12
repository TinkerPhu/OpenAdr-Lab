# Implementation Plan: VEN Controller Dashboard V2

**Branch**: `001-controller-dashboard-v2` | **Date**: 2026-03-14 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/001-controller-dashboard-v2/spec.md`

## Summary

Build a new "Controller V2" page in the VEN UI that displays all configured energy assets as vertically stacked cells, each showing real-time metrics (power/cost/CO₂eq), a timeline graph (past/present/future), and simulation controls. Two grid-level cells (Tariff, Accumulated Asset Power) sit above the asset cells. The page is scrollable with cell pinning. All data is sourced from existing VEN API endpoints; three stub fields are added to `UserOverrides` in the VEN backend to support direct SoC set and battery capacity override.

## Technical Context

**Language/Version**: TypeScript 5.x / React 18 (UI); Rust stable (VEN backend stubs only)
**Primary Dependencies**: recharts v2.15 (charts), MUI v5 (layout/controls), TanStack React Query v5 (data), React Router v6 (routing)
**Storage**: N/A — all data read from VEN API; pin/collapse state is ephemeral (React state, reset on reload)
**Testing**: Vitest + @testing-library/react (unit), Python behave BDD (integration), Playwright (E2E)
**Target Platform**: Browser (served from VEN nginx on port 8214), VEN API on ports 8211–8213
**Project Type**: Web application — new React page + minimal Rust backend stubs
**Performance Goals**: Dashboard renders within 2 seconds on initial load; simulation control changes visible within 5 seconds (SC-003)
**Constraints**: No changes to existing `/controller` page; VEN API sole data source; BDD-first; Pi4-Server builds only
**Scale/Scope**: 4 asset types (EV, Heater, PV, Battery) + BaseLoad per VEN; 3 VEN instances; up to 10 cells

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Notes |
|-----------|--------|-------|
| I. OpenADR Spec Fidelity | ✅ PASS | No OpenADR field names involved. All fields are VEN-internal API names (`power_kw`, `import_price_eur_kwh`, etc.) |
| II. BDD-First Testing | ⚠️ REQUIRED | Feature files must be written before any implementation code. 4 feature files planned under `tests/features/controller/` |
| III. Upstream Compatibility | ➖ N/A | No openleadr-rs submodule changes. Backend stubs are in VEN/src/state.rs only |
| IV. Lean Architecture | ✅ PASS | Reuses all existing hooks, recharts, MUI patterns. Three new stub fields in UserOverrides. No new HTTP routes. New components scoped to `controller/` subdirectory |
| V. Infrastructure Parity | ⚠️ REQUIRED | All builds/tests on Pi4-Server via SSH. Rebuild test-runner image whenever feature files or VEN source change |

*Post-design re-check*: All principles remain satisfied after Phase 1 design. No Complexity Tracking entries required.

## Project Structure

### Documentation (this feature)

```text
specs/001-controller-dashboard-v2/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/
│   └── ui-components.md # Phase 1 output
└── tasks.md             # Phase 2 output (/speckit.tasks — NOT created here)
```

### Source Code

```text
VEN/ui/src/
├── pages/
│   └── ControllerV2.tsx                      # New page, route /controller
├── components/
│   └── controller/
│       ├── types.ts                           # AssetId, AssetTimePoint, PinnedState, etc.
│       ├── dataBuilders.ts                    # buildAssetTimeline(), buildStackedAreaData()
│       ├── AssetCell.tsx                      # Full 3-section asset cell (composes left/mid/right)
│       ├── AssetLeftSection.tsx               # Metrics: power, cost, CO₂, SoC, user request
│       ├── AssetMidSection.tsx                # Timeline graph wrapper
│       ├── AssetRightSection.tsx              # Accordion groups: Status + Sim Characteristics
│       ├── GridTariffCell.tsx                 # Tariff + grid power left/right cell
│       ├── GridAccumulatedCell.tsx            # Stacked asset power cell
│       ├── PinnedZone.tsx                     # Sticky viewport area holding pinned cells
│       └── charts/
│           ├── AssetTimelineChart.tsx         # Multi-line (solid/dashed/dotted) per-asset chart
│           ├── TariffChart.tsx                # Import/export tariff + CO₂ + cost + grid power
│           └── StackedAreaChart.tsx           # Bidirectional stacked area (pos above, neg below)
└── __tests__/
    └── ControllerV2.test.tsx                 # Unit tests for page + all components

VEN/src/
└── state.rs                                   # +3 fields: ev_initial_soc, battery_initial_soc,
                                               #  battery_capacity_kwh to UserOverrides

tests/features/controller/
├── 01_layout.feature                         # Page structure, cells ordering, scrollability
├── 02_asset_cells.feature                    # Left/mid content, graph lines, sign convention
├── 03_simulation_controls.feature            # Right section controls, POST /sim/override
└── 04_navigation.feature                     # Pin, unpin, collapse, expand
```

**Structure Decision**: Web application (frontend-only, plus minimal backend stubs). New components in dedicated `controller/` subdirectory; BDD tests under `tests/features/controller/` as a grouped set. Existing `/controller` page is untouched.

## Complexity Tracking

*No constitution violations introduced. All complexity is proportional to feature scope.*
