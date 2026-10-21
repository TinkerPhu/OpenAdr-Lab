---
title: Opportunistic Objective (dispatch without a plan)
type: component
created: 2026-10-21
updated: 2026-10-21
synced_commit: f8f6775
sources: [docs/architecture/VEN_ARCHITECTURE.md, VEN/src/controller/arbiter/, VEN/src/simulator/, tests/features/ven_opportunistic_objective.feature]
tags: [objective, arbiter, baseline, fleet]
---

# Opportunistic Objective (dispatch without a plan)

`PlannerObjective::Opportunistic` is the common non-planning HEMS — surplus is absorbed, storage covers
import, requests run at full power — run on the same site so the lab can **compare it with the MILP
planner** ([[milp-planner]]). The comparison is operated fleet-wide through the objective command of
[[fleet-monitoring]].

## One switch, one step

`PlannerObjective::dispatches_from_plan()` is the only switch. When false the tick takes its
opportunistic arm and the planning cycle *forecasts* instead of solving; nothing a planning objective
does changes (VEN_ARCHITECTURE §2.1, §2.3.3). The decision is one function,
`controller::arbiter::opportunistic_step`, used live and in the forecast:
`request_regime` (a start-now request runs at full power and leaves the pass), `opportunistic_pass`
(site balance shared out open-loop in each asset's declared `surplus_claim_rank`), then the shared
`dispatch_stages` (dispatch window, comms-loss clamp, limit pass) that every objective goes through, so
limits are respected identically ([[deviation-arbiter]]).

## Forecast instead of a solve

`milp_planner::plan_from_forecast` hands the slot grid and the PV and base-load series to
`simulator::opportunistic_forecast`: a clone of the site stepped slot by slot with the same step. Each
asset reads its part back itself (`AssetMilpContext::solution_from_trace`). The plan has status
`SIMULATED`, is always adopted, and carries marginal values per slot, so the UI and the fleet plan feed
([[fleet-monitoring]]) treat it like any plan. Coarse slots smooth what live dispatch does second by
second; an export limit is not applied in the forecast (PV is not curtailed in it).

## Requests

Each request mode declares how it is served without look-ahead (`non_planning_dispatch`); an EV target
or a heater start-now reaches the asset as a per-asset tick input and the asset says when it is done
([[hems-planning]], [[asset-layer]]). Switching back writes no leftover lever state, so the first planned
tick decides as a fresh planning VEN. Tests: `controller/arbiter/tests/opportunistic_tests.rs`,
`tests/features/ven_opportunistic_objective.feature`.
