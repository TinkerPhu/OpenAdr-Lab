---
title: "Query: what is the Planner tab in the VEN UI for?"
type: query
created: 2026-07-17
updated: 2026-07-17
synced_commit: f068d94
sources: [VEN/ui/src/pages/Planner.tsx, VEN/ui/src/App.tsx, VEN/ui/src/pages/Controller.tsx, VEN/ui/src/pages/Dashboard.tsx, VEN/ui/src/components/sessions/SessionProgressBoard.tsx]
tags: [ui, planner, ux]
---

# Query: what is the Planner tab for — and should it be dismantled?

**Question:** What use does the Planner tab in the VEN UI have, from the VEN user's view
and from the debugging/understanding view? Could it be improved? Should it be dismantled
and spread across other tabs?

**Answer (short):** It is the window into the [[milp-planner]]'s *decision process* — the
"brain view". Every other tab shows the plan's effects; this one shows why the plan
exists. Keep it as the diagnostic tab; don't dismantle it — but surface its two genuinely
user-facing elements elsewhere. Improvement suggestions are filed as **BL-36, BL-37,
BL-38** in `docs/BACKLOG.md`.

## Composition (`VEN/ui/src/pages/Planner.tsx`)

| Element | Shows | Audience |
|---|---|---|
| Objective selector + weight legend | The one real **control**: min_cost / GHG / grid / autarky / revenue, with exact solver weights | user |
| PlannerStatusBar (SSE) | Live solve progress, then "Plan updated (trigger) — solved in N s" | both |
| PlanHeaderBar | Plan metadata: created when, triggered by what | debug |
| PlanPowerStack | Stacked per-asset power over the plan horizon | both |
| PlanTriggerTimeline | History of *why* replans fired | debug |
| PlanDecisionMatrix | Per-slot decisions and rates, incl. hatched estimated-rate slots ([[ven-ui]] WP4.6) | debug |
| SessionProgressBoard | Session/deadline progress ("EV charged by 7?") — [[hems-planning]] sessions | user |
| TraceTable (collapsed accordion) | Raw decision-trace event log | debug |
| CorrectionBanner (snackbar) | Live Layer-1 reactive battery correction ([[dispatcher]]) | both |

## Value by audience

- **VEN user (site operator):** only the objective selector (the most consequential user
  decision in the UI), the SessionProgressBoard, and loosely the PowerStack matter. The
  rest is noise to this persona.
- **Debugging/understanding:** the tab's real strength is *correlation* — diagnosing
  "why did the plan change?" needs trigger history + solve status + resulting matrix +
  trace side by side. The SSE status bar and correction banner are the only live views of
  the planner loop and the reactive-correction layer.

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
the EnergyPacket abstraction (Phase D, commits `efd861f`…`0079a77`), so it permanently
rendered its empty state. Same day, it was rebuilt UI-only as **`SessionProgressBoard`**
(`VEN/ui/src/components/sessions/SessionProgressBoard.tsx`, commit f068d94) on the live
[[hems-planning]] session vocabulary: `GET /user-requests` + live sim snapshot (EV SoC
fill gauge, heater current→target temperature) + the active plan (`planned_kw_by_asset`
summed to the session deadline vs `envelopes.energy_needed_kwh` → on-track/at-risk chip).
The budget bar shows `estimated_cost_eur` labeled "est." — per-session *accumulated* cost
does not exist anywhere (tracked as BL-39). The table above reflects the rebuilt board.

## Improvements → backlog

Filed in `docs/BACKLOG.md` (User-Value View, "comfort, control & trust"):

- **BL-36 — resolved (f068d94)** with the rebuild: condensed session chips + a read-only
  objective chip on the Dashboard (`dash-session-strip`); the objective *control* stays on
  the Planner tab beside its weight legend, where the legend and live solve feedback teach
  the user what each objective does.
- **BL-37** — route `correction_active`/`correction_cleared` through the backend
  [[notifications]] feed: `usePlannerEvents` subscribes only while the Planner page is
  mounted, so corrections firing on any other tab are currently invisible.
- **BL-38** — layout split (user zone on top, diagnostics collapsed below) and
  matrix-slot → trace filtering to close the "what happened in slot 14:35?" loop.
- **BL-39** — per-session accumulated-cost accounting so the budget bar can show real
  spend instead of the plan-time estimate (spun off from the BL-36 resolution).

The [[ven-ui]] page now carries the Planner-tab composition and Dashboard session strip
(coverage-gap item of 2026-07-17 resolved).
