# Proposal

## Why

A fleet-wide objective switch (`opportunistic` against `min_cost`) is the cleanest experiment the lab has:
the same 20 VENs, one condition, then the other. The Fleet page already draws the plan from before the last
switch faintly behind the current one, but gives no numbers, no replan progress, and no accumulated view.
The tariff page has all three (`PlanComparison`). The quickest honest reading of "what did the condition
change" is the accumulated planned energy: each VEN's plan shows its effect as soon as that VEN has
replanned, and the cumulative curves separate long before the hours they cover have happened.

## What Changes

- The Fleet plan card uses the same comparison as the Tariff page (`PlanComparison`): replan progress,
  provisional marking, import and export energy and peak before against now, over the window both plans cover.
- The plan chart can show the plan as accumulated energy (kWh from the plan's start), not only power, and
  opens on that view. One toggle, in the chart itself, so both pages get it.
- The instant a fleet switch started is read from the objective changes the BFF already notes: the first
  change of the newest switch, not the last VEN's, so the reference is the plan before any VEN switched.

## Capabilities

### New Capabilities
- `fleet-plan-comparison`: accumulated planned energy and the before/after comparison for any change
  of conditions (a published signal, an objective switch).

### Modified Capabilities
<!-- `fleet-reaction-comparison` (tariff-editor-compare-and-chart) is implemented and documented; this
generalises its component rather than changing its requirements. -->

## Impact

- `VTN/ui/src/`: `utils/planDelta.ts` (cumulative energy), `utils/fleetPlan.ts` (rows by quantity),
  `utils/fleetObjective.ts` (switch instant), `components/FleetPlanChart.tsx`, `components/PlanComparison.tsx`,
  `pages/Fleet.tsx`.
- `VTN/bff/`, VEN, wire: none. Reads `GET /api/fleet/plan?at=` and the objective change times as today.
