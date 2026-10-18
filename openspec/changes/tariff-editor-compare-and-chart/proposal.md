# Proposal

## Why

The workbench (`tariff-editor-workbench`) lets a user publish a tariff and then look at the
fleet power chart, but the user must remember what the fleet did before. Judging a tariff change
needs the before and after side by side. Typing numbers in a grid is also a slow way to shape a
curve; dragging steps on the chart is the natural second input mode over the same draft.

## What Changes

- Before/after comparison of the fleet's *planned* import and export: the plan as of just before the
  publish instant (faint) against the plan as of now (solid), with a per-direction delta and "n of m
  VENs replanned". Read from the new fleet plan feed (`fleet-plan-feed`), so nothing is stored by the
  page and a reload reproduces it.
- Drag-on-chart editing: drag a step's value, drag a band edge, with snap to the registry's grid
  and a numeric readout, editing the same draft as the grid.
- Templates and operators over the draft (flat, 2-tier, 3-tier; scale, shift, cap, paint range).
  Operators are pure functions on the draft, each defined once.
- A recurring draft stays one pattern: dragging a step edits the pattern, and every repetition
  in the preview moves with it.

## Capabilities

### New Capabilities
- `fleet-reaction-comparison`: frozen reference, delta and replan-state display for a published
  signal change.
- `signal-chart-editing`: pointer editing and operators over the signal draft.

### Modified Capabilities
<!-- none; depends on signal-workbench from tariff-editor-workbench -->

## Impact

- `VTN/ui/src/`: chart overlay layer for pointer editing (recharts has no draggable series),
  operator module, snapshot storage; `Tariff.tsx` gains the comparison panel.
- `VTN/bff/`: none; it reads `GET /api/fleet/plan?at=` from `fleet-plan-feed`.
- Depends on: `tariff-editor-workbench` (merged) and, for the comparison only, `fleet-plan-feed`.
