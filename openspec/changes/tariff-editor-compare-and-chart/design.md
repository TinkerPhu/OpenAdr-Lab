## Context

See proposal.md - Why. Depends on `tariff-editor-workbench` (draft model, registry, recurrence; merged) and
on `fleet-plan-feed` (`GET /api/fleet/plan?at=`; to be implemented first). Found while applying task 1.1:
the VTN has no view of VEN plans today, only actual power (`/api/fleet/power`) and per-event
reactions (`/api/fleet/reactions`); the plan feed closes that, and the comparison reads from it.

## Goals / Non-Goals

**Goals**: a before/after view of *planned* fleet import/export that survives a reload and needs no
storage of its own; a second input mode (drag) over the same draft; operators defined once.
**Non-Goals**: a dry run before publishing (needs a VEN planner endpoint); GHG and envelope editing;
comparing actual power (the existing Fleet power chart stays as it is).

## Inventory of existing logic reused or consolidated

| Concept | Where | This change |
|---|---|---|
| Planned fleet import/export as of an instant | `fleet-plan-feed`: `plan_at` / `GET /api/fleet/plan?at=` | The only source for reference and result; no second summing |
| Publish instant | the event's `createdDateTime` on the VTN | Used; no stored publish time |
| Chart window and tick helpers | `fleetChartWindow.ts` | Reused |
| Series merge, locf, clip | `@lab/ui/mergeSeries`, `fleetTariff.ts::tariffRows` | Reused for reference vs result rows |
| Draft mutation | `signalDraft.ts` (`setValue`, `fillRange`, `pasteRows`) | Operators and drag call these |
| Energy / peak of a series | not found in the VTN UI (searched `utils/`, `components/`) | One new function `seriesStats` in `utils/planDelta.ts` |
| Ranking VENs that replanned | `/api/fleet/reactions` | Not used here: it is keyed by event and reports 1-minute means; the plan feed's per-VEN `created_at` answers "replanned after the publish" directly |

## Decisions

**D1 Reference by recomputation.** `GET /api/fleet/plan?at=<publish − 1 s>` is the reference, `at=now`
the result. Both come from stored plans, so a reload reproduces them and nothing is saved by the page.
Alternative: freeze a copy in the browser or BFF at publish — rejected: lost on another device, and a
second copy of a number the store already holds.

**D2 Replan progress from the plan feed.** "Replanned" = the VEN's contributing plan at now has
`created_at` after the publish instant. The response of `plan_at` already lists each VEN's plan id and
creation time. Targeted VENs: the event's targets, or all known VENs when none.

**D3 Pointer layer is an SVG overlay over the recharts plot area**, mapping pixels to (time, value)
with the chart's own scales. Alternative: replace recharts for this chart — rejected (shared
`FleetTariffChart` behaviour and tests).

**D4 Operators as a table.** `signalOperators.ts` exports a record of `{label, params, apply}` and the
toolbar renders from it (`declare-dont-branch`). Every operator is a pure `SignalDraft -> SignalDraft`
that leaves the recurrence unchanged.

**D5 A drag on a repeated day edits the pattern.** The preview marks which repetition a band belongs to
(index of the pattern interval); the drag handler edits that pattern interval, so all repetitions move.

## Risks / Trade-offs

- [Overlay misaligns after resize] -> recompute from the chart's scales on resize; test.
- [Delta confounded by PV and weather forecast changes between the two plans] -> the panel shows both
  plan creation times and states that forecasts also move plans.
- [Clock skew between VEN and VTN puts a plan on the wrong side of the publish instant] -> compare
  VEN `created_at` with the event's `createdDateTime` and show both instants.
- [Drag precision] -> the grid remains the exact path; snapping to the registry step.

## Open Questions

1. Is a same-time-yesterday baseline wanted in addition? Does not change specs; adds one task if yes.
