---
title: VEN UI
type: component
created: 2026-07-04
updated: 2026-07-04
synced_commit: 4695762
sources: [VEN/ui/src, docs/history/project_journal.md]
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

Testing: Vitest + React Testing Library component tests, `data-testid`/`aria` attributes
per `docs/guidelines/REACT_GUIDELINES.md`; part of suite 1 in [[testing-strategy]].
