---
title: MILP Planner
type: component
created: 2026-07-04
updated: 2026-07-11
synced_commit: b1aba12
sources: [docs/architecture/ven_milp_planner.md, VEN/src/controller/milp_planner/, VEN/src/controller/milp_interactions.rs, VEN/src/controller/solver_port.rs, VEN/src/tasks/planning.rs, VEN/src/services/planning.rs, openspec/specs/two-phase-milp/spec.md, openspec/specs/planner-config/spec.md]
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

- **Grid-signal import caps** (Phase 3): alerts, SIMPLE levels 1–3, and capacity
  subscription+reservation allowances all converge on the per-slot contractual
  import cap (`p_imp_max_cont_kw` in `inputs.rs`) — alert → 0 (overrides all),
  SIMPLE L1 → `simple_level1_import_cap_pct` × contract, L2 → baseline forecast,
  L3 → 0, reservation allowance → min with the limit. The cap is a *soft*
  constraint (slack + violation penalty, warned in the plan), so no signal
  combination can make the solve infeasible and user deadlines yield
  automatically. See [[openadr-interface]] for the parsing side.
- **Plan grid**: 3-tier variable-step zones, `now` truncated to the Zone-A boundary —
  the full reasoning (gate stability, warm-start continuity, block-commitment anchor)
  is in [[three-tier-plan-grid]].
- **Sessions as constraints**: `EvSession`/`HeaterTarget`/`ShiftableLoad` enter the MILP
  as energy targets, deadline steps, and `MilpLoadMode` (MustRun/MayRun/MustNotRun) —
  the solver iterates over asset variables, never session objects
  (docs/architecture/VEN_ARCHITECTURE.md §2.3.1); see [[hems-planning]].
- **Adoption gate** (`VEN/src/services/planning.rs`): a periodic plan is adopted only if
  it beats the current plan's cost+friction by the effective threshold plus an optional
  per-extra-heater-switch surcharge (`gate_switch_penalty_eur`); hard triggers, a fully
  decayed threshold, or a current plan whose slots have all expired always adopt. Gate
  decay must measure real age, so it always receives `wall_now`, never the aligned
  timestamp (ven_milp_planner.md §2.2).
- **Stale-rate behaviour**: tariffs reach the solver as Step/LOCF `TariffTimeSeries`
  ([[tariffs-and-capacity]]) — the last known rate carries forward indefinitely, and
  hardcoded defaults (0.25 €/kWh import, 0.08 export, 300 g/kWh CO₂) fill slots with no
  data at all (`inputs.rs:77-90`). The four-variant `StaleRatePolicy` enum sketched as
  future roadmap is quarantined (unwired, not deleted) in `entities/design_vocabulary.rs`
  — `docs/BACKLOG.md` BL-07 tracks wiring `StaleRatePolicy::LastKnown` and
  `rate_estimated` (currently hardcoded `false`) as a real feature.
- **Asset isolation**: asset physics enter as `Vec<Box<dyn AssetMilpContext>>` — the
  planner never imports concrete asset types ([[ven-hexagonal-architecture]]).
  `tasks/planning.rs` reaches the solver through the `SolverPort` trait
  (`controller/solver_port.rs`), not `run_planner()` directly — `MilpSolver`
  (`milp_planner/mod.rs`) is the real implementation, and `services::PlanningService::solve_plan`
  is the only caller of `SolverPort::solve`. The actual HiGHS call still runs inside
  `spawn_blocking` (MILP solving takes 18–60 s on Pi4; the sim mutex is cloned and
  released first) — the port adds a swappable seam, not a different execution model.
- **Cross-asset interactions** (`controller/milp_interactions.rs`): pluggable
  `AssetInteraction` objects add coupled terms — `BatEvCoexist` (McCormick-linearised
  penalty on battery discharge during PV-covered EV charging) and `CtrlImportMalus`
  (slack penalty when controllable load exceeds free PV surplus). Active only when their
  coefficient is non-zero.
- **Heater anchor**: after adoption, the current heater block's tier binaries are pinned
  for the next solves until the block ends (`services/planning.rs::build_heater_anchor`,
  `anchor_until` in state) — prevents near-future chattering; hard triggers clear the
  anchor. Off-blocks are never anchored (would drive the tank below its domain bound
  and make the MILP infeasible).
- **Terminal energy rewards**: battery and heater get an end-of-horizon stored-energy
  credit auto-computed from the mean import tariff (battery: × round-trip efficiency;
  heater: + ctrl-import malus), profile-overridable — stops the optimizer from draining
  storage right before the horizon edge (`tasks/planning.rs:185-224`).
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
| Entry point (`run_planner`) + `SolverPort` impl (`MilpSolver`) | `VEN/src/controller/milp_planner/mod.rs` |
| `SolverPort` trait + `SolveRequest` | `VEN/src/controller/solver_port.rs` |
| Input tensors | `inputs.rs` |
| Weights, `MilpInputs`, `SolveOutput` | `types.rs` |
| Asset port (trait + var/context structs) | `asset_port.rs` |
| Phase 1 / Phase 2 | `solver_phase1.rs` / `solver_phase2.rs` |
| Cross-asset interactions | `VEN/src/controller/milp_interactions.rs` |
| Plan translation + fallback plan | `results.rs` |
| Per-session flexibility envelopes | `envelopes.rs` |
| Planning loop | `VEN/src/tasks/planning.rs` |
| Acceptance gate + heater anchor | `VEN/src/services/planning.rs` |

Downstream, the plan is executed slot-by-slot by the [[dispatcher]].
