---
title: MILP Planner
type: component
created: 2026-07-04
updated: 2026-07-04
synced_commit: eb8831a
sources: [docs/architecture/ven_milp_planner.md, VEN/src/controller/milp_planner/, VEN/src/tasks/planning.rs, VEN/src/services/planning.rs, openspec/specs/two-phase-milp/spec.md, openspec/specs/planner-config/spec.md]
tags: [planner, milp, highs, optimization]
---

# MILP Planner

The VEN's planning engine: a **two-phase Mixed-Integer Linear Program** solved by HiGHS
via the `good_lp` crate, producing a 48 h asset allocation plan on every replanning cycle
(docs/architecture/ven_milp_planner.md §1). It replaced the earlier greedy scheduler —
see [[milp-over-greedy]].

## Two phases

1. **Cost minimisation** — minimises import cost and CO₂ under capacity limits and
   EV/heater deadlines (`solver_phase1.rs`).
2. **Friction minimisation** — minimises relay switches and ramp changes while staying
   within `phase2_epsilon_eur` of Phase 1's optimum; warm-starts from Phase 1
   (`solver_phase2.rs`).

## Key mechanics

- **Plan grid**: 3-tier variable-step zones, `now` truncated to the Zone-A boundary —
  the full reasoning (gate stability, warm-start continuity, block-commitment anchor)
  is in [[three-tier-plan-grid]].
- **Sessions as constraints**: `EvSession`/`HeaterTarget`/`ShiftableLoad` enter the MILP
  as energy targets, deadline steps, and `MilpLoadMode` (MustRun/MayRun/MustNotRun) —
  the solver iterates over asset variables, never session objects
  (docs/architecture/VEN_ARCHITECTURE.md §2.3.1); see [[hems-planning]].
- **Adoption gate** (`VEN/src/services/planning.rs`): a new plan is adopted only if it
  beats the current plan's expected cost by a configured threshold — prevents churn.
  Gate decay must measure real age, so it always receives `wall_now`, never the aligned
  timestamp (ven_milp_planner.md §2.2).
- **StaleRatePolicy**: with the VTN unreachable, future tariff slots fall back to
  `LAST_KNOWN`, `HEURISTIC_FORECAST`, `DEFER_TO_FLEXIBLE`, or `SAFE_AVERAGE`.
- **Port isolation**: reached via `SolverPort`; asset physics enter as
  `Vec<Box<dyn AssetMilpContext>>` — the planner never imports concrete asset types
  ([[ven-hexagonal-architecture]]).
- **Phase 2 is a hard-bounded lexicographic pass, not a weighted blend**: it adds the
  constraint `phase1_cost ≤ C* + phase2_epsilon_eur` and then minimises switching/
  startup/ramp/tier-preference terms only — never trades cost for friction beyond that
  epsilon. Setting `phase2_epsilon_eur: 0.0` disables Phase 2 entirely (single-pass
  Phase 1 only). If Phase 2 comes back infeasible, the planner logs
  `"phase2 infeasible, falling back to phase1"` and adopts the Phase 1 schedule directly
  rather than crashing (`openspec/specs/two-phase-milp/spec.md`).
- **Initial-slot pinning**: slot 0's heater mode variables (`z_heat_mid[0]`,
  `z_heat_full[0]`) are fixed to the live heater's actual power state at planning time, so
  `sw[0]` — and its Phase 2 switching penalty — reflects a real transition, not a solver
  artifact of an unconstrained first slot.
- **Adoption threshold decay**: `plan_adoption_decay_s` (default 0, no decay) linearly
  decays `plan_adoption_threshold_eur` to zero as the current plan ages, so a plan that
  once looked "good enough" doesn't block replans indefinitely as conditions drift.
- **`solver_timeout_s`** (profile field, default 60 s) bounds the HiGHS time limit for
  both phases — see [[reliability-and-config]] for this and the other profile-driven
  config knobs.

## File map

| Concern | File |
|---|---|
| Entry point | `VEN/src/controller/milp_planner/mod.rs` |
| Input tensors | `inputs.rs` |
| Phase 1 / Phase 2 | `solver_phase1.rs` / `solver_phase2.rs` |
| Plan translation | `results.rs` |
| Planning loop | `VEN/src/tasks/planning.rs` |
| Acceptance gate | `VEN/src/services/planning.rs` |

Downstream, the plan is executed slot-by-slot by the [[dispatcher]].
