## Context

See proposal.md - Why. Depends on the draft model, registry and recurrence mapping from
`tariff-editor-workbench` (D1–D3 there). Fleet power comes from the existing fleet views
(`FleetPowerChart.tsx`, `fleetChartWindow.ts`); the VTN has a history store.

## Goals / Non-Goals

**Goals**: a before/after view that survives a reload; a second input mode with no second state.
**Non-Goals**: a true dry-run of VEN plans (would need a VEN planner endpoint — separate
feature); GHG/envelope.

## Inventory of existing logic reused or consolidated

| Concept | Where | This change |
|---|---|---|
| Chart window / x-axis | `fleetChartWindow.ts` | Reused |
| Merging timestamped series, locf | `@lab/ui/mergeSeries` | Reused for reference vs live |
| Draft mutation | `signalDraft.ts` | Operators and drag call the same functions |
| Fleet power history | to be confirmed in task 1.1 (history store / BFF fleet route) | Reused; no new sampler |

## Decisions

**D1 Reference stored server-side, not in the browser.** On publish the page asks the BFF for
fleet power over the window and stores it with the publish time in the existing BFF store, keyed by
the new event id. Alternative: `localStorage` — rejected, lost across devices and invisible to
Claude/experiments. Alternative: reconstruct "before" from history after the fact — viable when
history covers the window, preferred if task 1.1 confirms it, since it needs no new storage.

**D2 Replan status from plan timestamps.** "n of m replanned" compares each targeted VEN's latest
plan time to the publish time, using data the fleet view already fetches.

**D3 Pointer layer is an SVG overlay over the recharts plot area**, mapping pixels to
(time, value) with the chart's own scales. Alternative: replace recharts for this chart — rejected
(shared `FleetTariffChart` behaviour, tests).

**D4 Operators as a table.** `signalOperators.ts` exports a record of `{label, params, apply}`;
toolbar renders from it (`declare-dont-branch`).

## Risks / Trade-offs

- [Overlay misaligns after resize] → recompute from the chart's scales on resize; test.
- [Delta confounded by weather/PV changes between before and after] → show window and publish
  time; compare against the same-time-of-day previous day as an optional baseline (open question).
- [Drag precision] → grid remains the exact path.

## Open Questions

1. Is a same-time-yesterday baseline wanted in addition to the frozen reference? Does not change
   specs; adds one task if yes.
