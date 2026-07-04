## Why

The MILP planner combines economic cost and operational friction (relay switching, battery startup, EV ramp) into a single weighted objective. This forces a pre-declared trade-off rate between incommensurable quantities — EUR of electricity vs. relay wear events — which cannot be stably calibrated. The result is that the switching penalty distorts cost optimisation: when near-equivalent schedules differ by one relay cycle (0.50 EUR), the solver alternates between them on consecutive runs, producing plan oscillation and a 1 EUR swing in reported objective value for plans with nearly identical initial conditions.

## What Changes

- **Replace single-pass MILP solve with two sequential solves (lexicographic optimisation)**
  - Phase 1: minimise economic cost only (grid energy, battery wear, GHG, violations, BatEvCoexist interaction)
  - Phase 2: fix Phase 1 cost as a hard constraint (`C* + ε`), then minimise operational friction (heater switching, battery startup/ramp, EV startup/ramp, heater tier preference)
- **Remove switching/startup/ramp terms from Phase 1 weights** — `lambda_sw`, `c_bat_startup_eur`, `c_bat_ramp_eur_kw`, `c_ev_startup_eur`, `c_ev_ramp_eur_kw`, `w_tier_penalty_eur` are no longer mixed into the cost objective
- **Battery wear stays in Phase 1** — it is kWh-proportional and directly shapes how much the battery cycles; removing it would allow unconstrained over-cycling in Phase 1
- **`plan_adoption_threshold_eur` now compares Phase 1 cost** (`C*`) rather than the composite `objective_eur`, making the threshold a meaningful EUR signal
- **`plan_adoption_decay_s` makes the threshold decay linearly over time** — as time flows and the planning window rolls forward, the absolute cost of a new plan cannot always beat an older plan; the threshold decays to zero after `decay_s` seconds so a fresh plan is always adopted eventually
- **`objective_eur` on the Plan struct reflects Phase 1 cost** — the pure economic cost, not a mixed-unit composite; Phase 2 friction cost stored separately for diagnostics

## Capabilities

### New Capabilities

- `two-phase-milp`: Two-pass lexicographic MILP solve — Phase 1 cost-optimal, Phase 2 friction-minimal subject to cost constraint. Asset-agnostic: works for any combination of heater, battery, EV, shiftable loads.

### Modified Capabilities

None — no existing spec files exist to delta against.

## Impact

- **VEN** (`VEN/src/controller/milp_planner.rs`): `solve_milp` refactored into `solve_phase1` + `solve_phase2`; `MilpWeights` split into `Phase1Weights` and `Phase2Weights`
- **VEN** (`VEN/src/profile.rs`): `PlannerConfig` gains `phase2_epsilon_eur` field (default 0.02); existing per-asset penalty fields remain but are Phase 2 only
- **VEN** (`VEN/src/entities/plan.rs`): `Plan.objective_eur` semantics change to Phase 1 cost; add `friction_eur` field for Phase 2 objective value
- **VEN** (`VEN/src/loops.rs`): `plan_adoption_threshold_eur` comparison switches to `plan.phase1_cost_eur`
- **No VTN, BFF, or UI changes required** — `objective_eur` is already surfaced in the Planner UI; meaning improves silently
- **No openleadr-rs changes required**
- **No OpenADR 3.1 spec constraints apply** — this is internal planner logic

## Non-goals

- Changing the set of objectives (MinCost, MinGhg, MinGrid, MaxRevenue, Custom) — these presets continue to govern Phase 1 weights only
- Adding new assets or new penalty types
- Changing the planning horizon, step size, or replan trigger logic
- UI changes to expose Phase 1/Phase 2 cost breakdown (may follow separately)
