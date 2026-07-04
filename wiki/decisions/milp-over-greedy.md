---
title: "Decision: MILP over Greedy Scheduling"
type: decision
created: 2026-07-04
updated: 2026-07-04
synced_commit: eb8831a
sources: [docs/architecture/ven_milp_planner.md, docs/history/project_journal.md, docs/architecture/heater_tank_milp_planning_model.md]
tags: [decision, planner, milp]
---

# Decision: MILP over Greedy Scheduling

The planner solves a two-phase Mixed-Integer Linear Program (HiGHS via `good_lp`) rather
than allocating time-slots greedily (docs/architecture/ven_milp_planner.md §1;
VEN_ARCHITECTURE.md §2.3). Greedy allocation cannot trade off coupled constraints
globally — see below.

## Why

- **Coupled constraints**: EV deadlines, battery SoC windows, heater thermal trajectories
  (docs/architecture/heater_tank_milp_planning_model.md) and capacity caps interact;
  greedy allocation cannot trade them off globally, a MILP can.
- **Friction as a first-class objective**: Phase 2 minimises relay switches/ramps at
  bounded extra cost (`phase2_epsilon_eur`) — hard to express greedily.
- **Exactness with an escape hatch**: HiGHS via `good_lp`; a `fallback_plan` path exists
  in `results.rs` when the solver fails.

## Costs accepted

- HiGHS/cmake build dependency — the reason local Rust builds require WSL
  ([[deployment-topology]]).
- Solver runtime pressure → mitigated by the variable-step grid
  ([[three-tier-plan-grid]]) and single-zone test profiles ([[testing-strategy]]).

Engine details: [[milp-planner]].
