---
title: "Query: what is the Planner tab in the VEN UI for?"
type: query
created: 2026-07-17
updated: 2026-10-22
synced_commit: 650b626
sources: [VEN/ui/src/pages/Planner.tsx, VEN/ui/src/App.tsx, VEN/ui/src/pages/Controller.tsx, VEN/ui/src/pages/Dashboard.tsx, VEN/ui/src/components/sessions/SessionProgressBoard.tsx, VEN/ui/src/components/planner/]
tags: [ui, planner, ux]
---

# Query: what is the Planner tab for — and should it be dismantled?

**Question:** What use does the Planner tab in the VEN UI have, from the VEN user's view
and from the debugging/understanding view? Could it be improved? Should it be dismantled
and spread across other tabs?

**Answer (short):** It is the window into the [[milp-planner]]'s *decision process* — the
"brain view". Every other tab shows the plan's effects; this one shows why the plan
exists. Keep it as the diagnostic tab; don't dismantle it — but surface its two genuinely
user-facing elements elsewhere. The user-facing elements sit on top and the
diagnostics are collapsed below.

## Composition (`VEN/ui/src/pages/Planner.tsx`)

The page heading reads "Plan". A user zone sits on top; everything that explains *why* is collapsed
into a "Diagnostics" accordion below it.

| Element | Shows | Audience |
|---|---|---|
| Objective selector + weight legend | The one real **control**: the objectives with their exact solver weights, and where the active one came from | user |
| PlannerStatusBar (SSE) | Live solve progress, then "Plan updated (trigger) — solved in N s" | both |
| PlanPowerStack | Stacked per-asset power over the plan horizon | both |
| SessionProgressBoard | Session/deadline progress ("EV charged by 7?") — [[hems-planning]] sessions | user |
| *Diagnostics accordion:* PlanHeaderBar | Plan metadata: created when, triggered by what | debug |
| PlanTriggerTimeline | History of *why* replans fired | debug |
| PlanDecisionMatrix | Per-slot decisions and rates, incl. hatched estimated-rate slots ([[ven-ui]]); clicking a slot filters the trace below to that slot's window | debug |
| TraceTable (nested accordion, event count in its title) | Raw decision-trace event log | debug |

## Value by audience

- **VEN user (site operator):** only the objective selector (the most consequential user
  decision in the UI), the SessionProgressBoard, and loosely the PowerStack matter. The
  rest is noise to this persona.
- **Debugging/understanding:** the tab's real strength is *correlation* — diagnosing
  "why did the plan change?" needs trigger history + solve status + resulting matrix +
  trace side by side. The SSE status bar is the only live view of the planner loop.

## Verdict: keep, don't dismantle

The debugging surfaces lose most of their value if scattered — trigger timeline without
the matrix, or trace without solve status, forces tab-hopping during exactly the
investigation they exist for. Overlap with other tabs is smaller than it looks:
Controller shows per-asset *timelines with zones* ([[three-tier-plan-grid]],
`VEN/ui/src/pages/Controller.tsx`), Dashboard shows *current state* (capacity, ledger,
`VEN/ui/src/pages/Dashboard.tsx`); neither duplicates the decision matrix or trace.

## Correction (2026-07-17) — the board was dead when this page was written

The original answer classified `PacketProgressBoard` as one of the tab's two user-facing
elements. It was in fact **dead UI**: it polled `GET /packets`, an endpoint removed with
the EnergyPacket abstraction (Phase D, commits `ba1ee92`…`b0543d7`), so it permanently
rendered its empty state. Same day, it was rebuilt UI-only as **`SessionProgressBoard`**
(`VEN/ui/src/components/sessions/SessionProgressBoard.tsx`, commit 9ba32e7) on the live
[[hems-planning]] session vocabulary: `GET /user-requests` + live sim snapshot (EV SoC
fill gauge, heater current→target temperature) + the active plan (`planned_kw_by_asset`
summed to the session deadline vs `envelopes.energy_needed_kwh` → on-track/at-risk chip).
The budget bar shows `estimated_cost_eur` labeled "est." — per-session *accumulated* cost
did not exist anywhere then. The table above reflects the rebuilt board.

## What became of the improvement ideas

- **Session strip on the Dashboard** (BL-36): condensed session chips and a read-only objective chip on
  the Dashboard (`dash-session-strip`); the objective *control* stays on the Plan page beside its weight
  legend, where the legend and live solve feedback teach the user what each objective does.
- **Layout split and slot → trace filtering** (BL-38): done — the user zone is on top, diagnostics are
  collapsed, and a decision-matrix slot filters the trace.
- **Reactive-correction banner** (BL-37): gone with the banner; reactive corrections are read from the
  arbiter's own diagnostics and decision events ([[deviation-arbiter]]).
