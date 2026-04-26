## Context

The VEN MILP planner (`VEN/src/controller/milp_planner.rs`) currently solves a single mixed-integer program where economic cost terms (grid energy, battery wear, GHG) and operational friction terms (heater relay switching, battery startup/ramp, EV startup/ramp) are combined into one weighted objective via `MilpWeights`. Because the switching penalty (0.50 EUR/relay event) is the same order of magnitude as the tariff-spread savings over a planning slot, the solver finds two structurally different heater schedules (e.g. two short PV-window runs vs. one longer cross-boundary run) as near-equivalent solutions. Consecutive planning cycles alternate between these, producing observable plan oscillation and a ~1 EUR swing in `objective_eur` for nearly identical initial conditions.

The `plan_adoption_threshold_eur` field exists in `PlannerConfig` to suppress churn, but compares the composite `objective_eur` (which includes switching penalties) — making it impossible to set a stable threshold in EUR of electricity savings.

## Goals / Non-Goals

**Goals:**
- Phase 1 always finds the cost-optimal schedule; switching penalties never distort it
- Phase 2 finds the smoothest implementation of that cost-optimal schedule
- `objective_eur` on the `Plan` struct becomes a meaningful pure-EUR cost figure
- `plan_adoption_threshold_eur` comparison uses Phase 1 cost exclusively
- All asset combinations (heater-only, battery+EV+heater, no controllable assets) work without special casing

**Non-Goals:**
- Changing planner objective presets (MinCost, MinGhg, MinGrid, MinImport, MaxRevenue, Custom)
- Exposing Phase 1 / Phase 2 cost split in the UI (deferred)
- Changing plan horizon, step size, replan trigger, or deviation correction logic

## Decisions

### Decision 1 — Split `solve_milp` into two functions, not one parametric function

**Options considered:**
- A) One function with a `phase: Phase` enum parameter that conditionally assembles the objective
- B) Two separate functions: `solve_phase1(inputs) -> Phase1Result` and `solve_phase2(inputs, c_star, epsilon) -> SolveOutput`
- C) A `solve_milp_two_phase(inputs, weights) -> SolveOutput` wrapper that does both internally

**Decision: C** — the wrapper is the cleanest public interface. `run_planner()` calls one function and gets one result; the two-pass logic is encapsulated. `solve_phase1` and `solve_phase2` become private helpers inside `milp_planner.rs`. This keeps the call sites in `loops.rs` and tests unchanged.

### Decision 2 — Phase 1 and Phase 2 reuse the same `MilpInputs`

Both phases receive the same `MilpInputs` struct. Phase 1 ignores all switching/startup/ramp penalty fields; Phase 2 uses them. No new input struct is needed.

**Alternative rejected:** Splitting `MilpInputs` into `Phase1Inputs` and `Phase2Inputs` — unnecessary coupling; the fields are cheap to carry and their presence is already conditional on asset availability.

### Decision 3 — Phase 1 cost constraint is expressed as an LP expression, not a stored scalar

In Phase 2 the constraint `phase1_cost ≤ C* + ε` is built by re-evaluating the same expression that Phase 1 minimised (grid energy + battery wear + GHG + violations) against Phase 2's newly declared variables. The scalar `C*` is returned from `solve_phase1` as a `f64`.

**Why not store the phase1 optimal solution and fix variables?** Fixing all Phase 1 decision variables would leave Phase 2 nothing to optimise — the switching/startup/ramp variables are in addition to the energy flow variables. Phase 2 must re-solve the full integer program with the cost pinned as a constraint; only the objective function changes.

### Decision 4 — `MilpWeights` split into `Phase1Weights` and `Phase2Weights`

```rust
struct Phase1Weights {
    w_energy: f64,
    w_ghg: f64,
    w_grid: f64,
    w_import: f64,
    w_viol: f64,
    c_bat_wear_eur_kwh: f64,
    c_bat_ev_coexist_eur_kwh: f64,  // stays Phase 1 — economic energy-flow signal
    w_services: f64,
}

struct Phase2Weights {
    c_bat_startup_eur: f64,
    c_bat_ramp_eur_kw: f64,
    c_ev_startup_eur: f64,
    c_ev_ramp_eur_kw: f64,
    lambda_heat_sw_eur: f64,        // from MilpInputs.lambda_heat_sw_eur
    w_tier_penalty_eur: f64,        // from MilpInputs.w_tier_penalty_eur
}
```

`build_milp_weights()` is replaced by `build_phase1_weights()` and `build_phase2_weights()`, both derived from `PlannerConfig` and the active `PlannerObjective`. The objective presets (MinCost etc.) continue to govern Phase 1 energy weights exclusively.

### Decision 5 — `Plan.objective_eur` = Phase 1 cost; add `friction_eur` field

`Plan.objective_eur` currently holds the composite score. After this change it holds only the Phase 1 economic cost — a pure EUR figure comparable across plan cycles. A new `friction_eur: f64` field holds the Phase 2 objective value (total switching/startup/ramp cost in EUR-equivalent). Both are populated by `run_planner()`.

This is a silent improvement to existing consumers: the Planner UI already displays `objective_eur`; the number becomes more meaningful without a UI change.

### Decision 6 — `phase2_epsilon_eur` added to `PlannerConfig`

```rust
/// Tolerance for the Phase 2 cost constraint: phase1_cost ≤ C* + epsilon.
/// Prevents Phase 2 infeasibility due to numerical precision.
/// Default: 0.02 EUR. Set to 0.0 to disable Phase 2 (single-pass behaviour).
#[serde(default = "default_phase2_epsilon")]
pub phase2_epsilon_eur: f64,
```

Setting `phase2_epsilon_eur = 0.0` disables Phase 2 entirely, restoring single-pass behaviour. This is the rollback/escape hatch.

### Decision 7 — Initial heater mode pinned at t=0

As a companion fix: the current MILP does not constrain `z_heat_mid[0]` / `z_heat_full[0]` to match the live heater state. A switch from the actual state to the plan's t=0 mode is unaccounted for. `HeaterMilpContext::from_state()` gains `initial_z_mid: f64` and `initial_z_full: f64` fields; `constraints()` adds:

```rust
cs.push(constraint!(v.z_heat_mid[0] == self.initial_z_mid));
cs.push(constraint!(v.z_heat_full[0] == self.initial_z_full));
```

This eliminates a source of free t=0 switching that distorts both phases.

### Decision 8 — Adoption threshold decays linearly with plan age

**Problem:** `plan_adoption_threshold_eur` compares the new plan's Phase 1 cost against the current plan's Phase 1 cost. Because the planning window rolls forward with time, a new plan covering a later (potentially more expensive) horizon cannot always produce a lower `objective_eur` than the current plan even if it is the genuinely optimal solution for current conditions. This means the threshold can permanently block plan updates when circumstances change (energy prices rise, capacity constraints tighten).

**Decision:** Add `plan_adoption_decay_s: f64` to `PlannerConfig` (default `0.0` = no decay). When `decay_s > 0`, the effective threshold applied at the adoption gate is:

```
elapsed_s = now − current_plan.created_at
decay_factor = clamp(1.0 − elapsed_s / decay_s, 0.0, 1.0)
effective_threshold = plan_adoption_threshold_eur × decay_factor
```

After `decay_s` seconds, `effective_threshold` reaches 0.0 and any new plan is accepted. This guarantees the controller stays responsive to genuine environmental changes over time.

**Default `0.0`** preserves backward-compatible always-no-decay behaviour. Users who set `plan_adoption_threshold_eur > 0` should also configure `plan_adoption_decay_s` (suggested: 5–10× `replan_interval_s`).

**Why linear?** Simplest decay shape; easy to reason about (threshold halves at `decay_s / 2`, reaches zero exactly at `decay_s`). Exponential decay never reaches zero, creating the same guaranteed-block problem in a weaker form.

## Risks / Trade-offs

**Solve time doubles** → Phase 1 is simpler (no switching binaries in objective, but same integer variables); Phase 2 has a tight cost constraint that prunes the search space heavily. On Pi4 ARM64, each phase should complete well within 10s for a 288-slot problem with heater+battery+EV. Total budget remains comfortably under the 60s time limit. If Pi4 performance is marginal, `phase2_epsilon_eur` can be raised to give Phase 2 more slack, or the Phase 2 time limit can be tightened independently.

**Phase 2 infeasibility** → If `C*` is the exact LP-relaxation bound (no integer feasible solution at that cost), Phase 2 would be infeasible. The epsilon (default 0.02 EUR) prevents this. Additionally: if Phase 1 terminates at the MIP gap rather than the global optimum, `C*` may be slightly optimistic. The epsilon absorbs this. Fallback: if Phase 2 returns an error, `run_planner` logs a warning and returns the Phase 1 solution directly (same behaviour as today minus switching optimisation).

**`plan_adoption_threshold_eur` semantics change** → The threshold now compares `plan.objective_eur` (Phase 1 cost) between current and new plan. Since Phase 1 cost is now a pure EUR figure, a threshold of 0.20 EUR is meaningful and stable. Existing deployments with `plan_adoption_threshold_eur: 0.0` are unaffected (always-adopt).

**Existing unit tests** → Tests that call `solve_milp` directly need updating to call the new two-phase wrapper. Tests that assert on `objective_eur` values need updated expected values (now pure Phase 1 cost, not composite).

## Migration Plan

1. Implement and unit-test `solve_phase1` / `solve_phase2` / `solve_milp_two_phase` in isolation
2. Wire `run_planner()` to call `solve_milp_two_phase`; update `Plan` struct fields
3. Update `loops.rs` adoption threshold comparison to use `objective_eur` (now Phase 1 cost)
4. Update unit tests; run full BDD suite on Pi4
5. Deploy to Pi4; observe planner logs for Phase 2 infeasibility warnings
6. Rollback: set `phase2_epsilon_eur: 0.0` in profile YAML to restore single-pass behaviour without code change

## Open Questions

- Should `friction_eur` be surfaced in the Planner SSE `PlanReady` event for UI display? (Deferred — not in scope for this change, but `friction_eur` on the Plan struct makes it available.)
- Should the Phase 2 time limit be independently configurable from Phase 1? (Current plan: share the same 60s limit; Phase 2 is expected to be much faster due to tight cost constraint.)
