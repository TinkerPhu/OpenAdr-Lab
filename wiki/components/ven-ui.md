---
title: VEN UI
type: component
created: 2026-07-04
updated: 2026-07-06
synced_commit: ae4a1ed
sources: [VEN/ui/src, docs/history/project_journal.md, VEN/src/routes/timeline.rs, VEN/src/controller/timeline.rs]
tags: [ui, react, timeline]
---

# VEN UI

React + TypeScript SPA (Vite build, nginx-served, port 8214) — the per-site dashboard for
[[openadr-lab]]'s VEN containers (docs/history/project_journal.md §6).

## Structure

- `src/api/client.ts` — `VenApi` fetch wrapper; `src/api/hooks.ts` — react-query hooks
  with `refetchInterval` polling; `src/api/types.ts` — pass-through DTO types
  ([[dto-pass-through]]).
- `VenContext` — multi-VEN selector switching all pages across the three instances.
- Pages: Dashboard, Programs, Events, Sensors; plus the planner timeline views exercised
  by `tests/features/ven_ui_planner.feature` and `ven_timeline.feature`.

## Timeline specifics

The timeline renders the plan produced by the [[milp-planner]] including its variable-step
zones ([[three-tier-plan-grid]]): zone shading uses per-zone opacity (fixed at 7edeb08 so
Zone A is not invisible), and a **now-point** marker shows the live simulator value at the
exact request time — deliberately *not* snapped to the plan grid
(docs/architecture/ven_milp_planner.md §2.2, timeline now-point).

`GET /timeline/:asset_id` and `/timeline/all` (`VEN/src/routes/timeline.rs`,
`VEN/src/controller/timeline.rs`) serve the chart data. The **future/forecast segment**
returns one real point per real plan slot at its native per-zone step size (5/10/15 min,
`build_asset_timeline`) — it is no longer resampled onto a fixed-width grid with
time-weighted averaging. That resampling used to blend real slot values into synthetic
buckets and desynchronise the displayed timestamp from any real planning decision
whenever the bucket width didn't line up with a zone's step size (routine in the
expanded 48 h view). The **history segment** is still grid-resampled at a fixed
resolution, since it has no natural "slot" structure to preserve. The frontend needed no
change: recharts' existing tooltip snap already reads real `ts` values from the data
array, so it now snaps to real plan-slot boundaries instead of fake grid buckets.

Testing: Vitest + React Testing Library component tests, `data-testid`/`aria` attributes
per `docs/guidelines/REACT_GUIDELINES.md`; part of suite 1 in [[testing-strategy]].
