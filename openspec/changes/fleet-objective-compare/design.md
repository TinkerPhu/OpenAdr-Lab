## Context

See proposal.md - Why. Everything needed is on the wire already: the stored plans answer "as of an instant"
(`GET /api/fleet/plan?at=`), and the BFF notes when each VEN's objective changed (`objectiveChangedAt`).

## Goals / Non-Goals

**Goals**: the fleet-switch experiment readable on the Fleet page with numbers, progress and an accumulated
view, using the component the Tariff page already has. **Non-Goals**: splitting the fleet into groups (the
experiment is the same 20 VENs under two conditions, one after the other), a new BFF route, storing results.

## Inventory of existing logic reused or consolidated

| Concept | Where | This change |
|---|---|---|
| Plan before and after a change, delta table, replan progress | `PlanComparison.tsx`, `utils/planDelta.ts` (`planDelta`, `replanProgress`) | Reused as is on the Fleet page; its wording takes the name of the change as a prop instead of saying "publish" |
| Energy of a series as kW held for a grid step | `planDelta.ts::seriesStats` | `seriesStats` and the new cumulative function share one conversion (`kw * stepSeconds / 3600`); no second copy |
| Instant before a change | `fleetPlan.ts::instantBefore` | Used for both the tariff publish and the switch |
| Latest objective change | `fleetObjective.ts::latestObjectiveChange` | Replaced by `latestObjectiveSwitch` (the start of the newest switch); its one caller is the Fleet page's faint overlay, which now goes through `PlanComparison` |
| Chart rows from a plan | `fleetPlan.ts::planRows` | Takes the quantity (power or accumulated energy); one row builder |
| Chart axis unit and formatting per quantity | per-chart literals today | Declared once per quantity in `FleetPlanChart` and looked up (`declare-dont-branch`) |

## Decisions

**D1 Accumulate on the grid, per direction.** Accumulated import and export are separate running sums of
`netImportKw` and `netExportKw` over the plan's slots, in kWh at the end of each slot. Not a net sum: a
site importing while another exports must not cancel, which is also why the plan states them apart.

**D2 Compare over the window both plans cover.** As in `planDelta`: a later plan reaches further ahead and
counting that as extra energy would report the horizon. The accumulated chart starts both curves at zero at
the same instant (the start of the window both cover), so the gap between them is the effect of the change.

**D3 The switch instant is the first change of the newest cluster.** The BFF switches a fleet one VEN at a
time, spaced apart, so the VENs' `objectiveChangedAt` differ by seconds to minutes. Changes less than
`SWITCH_GAP_MS` (10 minutes) apart belong to one switch; the reference is the plan before the earliest one,
and progress counts every VEN as targeted. Alternative: the last change (today's rule) - rejected: the
reference would already contain the VENs that switched first.

**D4 Open on accumulated.** The toggle defaults to accumulated energy, as the user asked: it is the view
that shows a result first. Power stays one click away.

## Risks / Trade-offs

Two switches within 10 minutes read as one. Forecast changes move plans too, not only the objective; the
caption already says so for the tariff and now says it for the switch.
