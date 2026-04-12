# Implementation Plan: Grid-Aligned UI Timeline

**Branch**: `011-grid-aligned-ui` | **Date**: 2026-03-21 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/011-grid-aligned-ui/spec.md`

## Summary

Adapt all VEN UI timeline consumers to work with the RF-05c grid-aligned backend response. The response shape is unchanged (`Record<string, {ts, values|null}[]>`), but `values` can now be `null` for empty grid buckets. The main change is replacing the tolerance-based `findNearest`/`TOLERANCE_MS` logic in `GridAccumulatedCell` with a simple positional zip, and ensuring all other consumers (`AssetCell`, `GridTariffCell`, `dataBuilders`, `RawDiagnostics`) handle `values: null` gracefully.

## Technical Context

**Language/Version**: TypeScript 5, React 18
**Primary Dependencies**: MUI v5, TanStack React Query v5, recharts, Vitest, @testing-library/react
**Storage**: N/A (read-only UI consuming backend API)
**Testing**: Vitest (unit), behave BDD (integration)
**Target Platform**: Browser (VEN UI served via nginx on port 8214)
**Project Type**: Web application (frontend only for this feature)
**Performance Goals**: N/A (no measurable performance change expected)
**Constraints**: Must not break existing Controller V2 page behavior
**Scale/Scope**: ~8 files modified, ~2 test files updated/created

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Notes |
|-----------|--------|-------|
| I. OpenADR Spec Fidelity | PASS | No OpenADR field names involved; timeline is internal VEN API |
| II. BDD-First Testing | PASS | Existing BDD scenarios for Controller V2 UI cover this; vitest unit tests will validate the new logic |
| III. Upstream Compatibility | N/A | No openleadr-rs changes |
| IV. Lean Architecture | PASS | Removing complexity (findNearest, TOLERANCE_MS). Only change is making `values` nullable in existing type. No new abstractions. |
| V. Infrastructure Parity | PASS | No Docker/infrastructure changes |

No violations. No Complexity Tracking entries needed.

## Project Structure

### Documentation (this feature)

```text
specs/011-grid-aligned-ui/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
└── tasks.md             # Phase 2 output (created by /speckit.tasks)
```

### Source Code (files to modify)

```text
VEN/ui/src/
├── api/
│   ├── client.ts                          # allTimelines(): handle values:null, add resolution param
│   └── hooks.ts                           # useAllTimelines(): pass resolution option
├── components/controller/
│   ├── types.ts                           # AssetTimelinePoint.values becomes nullable
│   ├── GridAccumulatedCell.tsx             # Remove findNearest/TOLERANCE_MS, positional zip
│   ├── dataBuilders.ts                    # computeForecastEnergy: skip null values
│   ├── tariffBuilders.ts                  # buildPowerPoints: handle null values
│   └── charts/
│       └── AssetTimelineChart.tsx          # Handle null values entries
├── pages/
│   ├── ControllerV2.tsx                   # No structural changes (allTimelines shape unchanged)
│   └── RawDiagnostics.tsx                 # Handle null values in raw display
└── __tests__/
    ├── GridAccumulatedCell.test.tsx        # Update for positional-zip approach
    └── dataBuilders.test.ts               # Add null-values test cases
```

**Structure Decision**: Frontend-only changes within existing `VEN/ui/src/` structure. No new directories or files needed (all edits to existing files).
