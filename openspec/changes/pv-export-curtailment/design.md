## Context

PV is currently a constant forecast array (`GlobalMilpInputs::p_pv_kw`, built in `inputs.rs` from
`PvInverter::build_milp_context`) added directly into the grid power-balance constraint in both
solver phases:

```
p_imp[t] + p_pv_kw[t] + bat_dis == p_base[t] + p_residual[t] + ev_kw + heat_kw + shift_kw + bat_ch + p_exp[t]
```

Separately, `PvInverter.export_limit_kw` is the field `step_inner` actually clamps against
(`raw_kw.max(lim)`, `lim` stored as a negative magnitude — "at most this much export"). Nothing in
the live tick pipeline (`tasks/sim_tick/`, `simulator/mod.rs`, `controller/dispatcher.rs`) writes
to it today: `OadrCapacityState.export_limit_kw` (set correctly from VTN `EXPORT_CAPACITY_LIMIT`
events in `openadr_interface.rs`) only reaches `dispatcher::build_setpoints`'s `setpoints["pv"]`
entry, and `PvInverter::step()` ignores its `setpoint_kw` argument (already flagged as dead code by
its `_setpoint_kw` naming). So curtailment is a full round trip today: modelled in the LP-adjacent
doc, half-wired through capacity state, and dropped before it reaches physics.

This design closes both gaps together: give the planner a real decision variable, and make its
(and the VTN's) output actually land on the asset.

## Goals / Non-Goals

**Goals:**
- Let the MILP choose to curtail PV export within `[0, forecast]` per slot when it relieves an
  active constraint (export cap, soft-violation penalty), with zero cost for curtailing itself.
- Make the resulting `export_limit_kw` value actually reach `PvInverter` every tick — fixing the
  pre-existing dead VTN-driven path in the same change, since both share the same sink.
- Expose the curtailment decision in `PlanSlot` and the VEN UI (ui-transparency rule).

**Non-Goals:**
- No new `AssetMilpContext` implementation for PV. PV has no startup/ramp/mode state — it is a
  pure clamp on a forecast, which is exactly what `GridMilpVars` (home of `p_imp`/`p_exp`) already
  models for the grid side. Adding a full per-asset context (declare_vars/constraints/objective/
  milp_params) would duplicate machinery built for stateful, schedulable assets onto something
  that isn't one.
- No new objective term / curtailment cost. See Decisions — the existing cost terms already
  produce the correct incentive (curtail only when forced) without one.
- No change to the §5 marginal-cost/shadow-price arbiter or `SolverPort` duals design
  (`deviation-scenarios-analysis.md` backlog tasks 2–3) — this change only adds the lever those
  tasks will later prioritize among; it does not build the arbiter.
- No VTN/openleadr-rs protocol change. `EXPORT_CAPACITY_LIMIT` parsing is untouched; this only
  fixes what happens to the value it already produces.

## Decisions

**1. `p_pv_used` lives in `GridMilpVars`, not a new `AssetMilpContext`.**
`GridMilpVars` already holds `p_imp`/`p_exp`/`u_grid` — grid-level variables with no asset-specific
scheduling semantics. `p_pv_used[t]` is the same kind of thing: one continuous variable per slot,
bounded by a per-slot input array, entering the balance equation and nothing else. Declared
alongside `p_imp`/`p_exp` in both `solver_phase1.rs` and `solver_phase2.rs`:
```
let p_pv_used: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();
...
model = model.with(constraint!(p_pv_used[t] <= inputs.p_pv_kw[t]));
```
and substituted for `inputs.p_pv_kw[t]` in the balance constraint.

Alternative considered: a full `AssetMilpContext` for PV (own `milp_params`/`declare_vars_into_pool`/
`constraints`/`objective`). Rejected — PV has no cross-slot state (no SoC, no ramp, no startup), so
every method but `constraints` would be a near-empty stub; the trait exists to give stateful assets
a uniform plug-in point, not to force every physical quantity through it.

**2. A tiny full-utilization tie-break is needed after all (revised during implementation).**
The original plan was "no objective term for curtailment" — every real cost term (tariff revenue
on `p_exp`, minus the small `w_grid` grid-exchange friction) already nets out in favor of using
PV, so curtailment should only happen when a real constraint forces it. That held for the *true*
LP optimum, but implementation surfaced a gap: `solve_phase1` calls `model.with_mip_gap(0.02)`
(a pre-existing, documented 2% relative tolerance HiGHS uses to accept a "good enough" incumbent
for the `u_grid` mutual-exclusion binary) — and a PV-only test with **no export cap at all**
(`pv_used_equals_forecast_when_no_export_constraint_binds`) still came back with PV curtailed by a
small, cost-irrelevant amount. Separately, Phase 2's objective is friction-only and has *no*
opinion on `p_pv_used` whatsoever — nothing stopped it from curtailing PV arbitrarily while
optimizing switching/ramp within the epsilon cost budget.

Fix: added `PV_USE_TIEBREAK_EUR_PER_KWH` (0.005 €/kWh) and `pv_use_tiebreak_expr()`
(`milp_interactions.rs`), applied in both phases exactly like the pre-existing
`SHIFT_TIEBREAK_EUR_PER_SLOT` pattern for shiftable-load start slots — small enough that any real
constraint (export cap, soft-violation penalty) still dominates and forces genuine curtailment,
but large enough to close the MIP-gap-tolerance and Phase-2-indifference gaps. This is the same
category of fix the codebase already has precedent for, not a new pattern.

Alternative considered: tightening `with_mip_gap` instead of adding a tie-break. Rejected — the gap
setting is shared with every other MIP decision in the same solve (heater/battery/EV switching),
so tightening it globally to fix one variable's degeneracy would slow every solve for a problem
that a targeted, tiny objective term already fixes cheaply.

**3. Runtime wiring goes through `SimState::tick()`, not the `setpoints` HashMap.**
`build_setpoints`'s `HashMap<String, f64>` is a *power target* contract (used for the battery,
EV, and eventually would-be PV dispatch) — `PvInverter::step()` correctly never reads it as a
target, since PV output isn't directly commandable, only clampable. Reusing that channel for an
export *ceiling* would conflate two different kinds of signal. Instead, `SimState::tick()` gains
`pv_export_limit_override: Option<f64>`, applied in the existing `AssetConfig::Pv(pv) => { ... }`
match arm alongside `irradiance`/`weather_power_kw`, the same place `Heater`'s
`apply_tick_overrides` already lives for its own per-tick overrides. `dispatcher.rs` (or a small
sibling function) resolves the single value to pass in as
`min_magnitude(capacity.export_limit_kw, plan_derived_limit_kw)` — either source can only tighten,
never loosen, so taking the smaller magnitude is correct without needing to know which one applies.

**4. `PlanSlot.pv_used_kw`, not a separate curtailment-only field.**
Store the solved `p_pv_used[t]` value directly (same shape as the existing `pv_forecast_kw`);
curtailment amount is a trivial `pv_forecast_kw - pv_used_kw` derivation, computed where needed
(UI, dispatcher) rather than duplicated as a second stored field.

## Risks / Trade-offs

- [Fixing the dead VTN wiring changes long-standing (if inert) behavior for
  `EXPORT_CAPACITY_LIMIT` events] → This is a bugfix, not a behavior change worth gating separately:
  the field has always been *intended* to clamp PV (its own docstring and `dispatcher.rs`'s comment
  say so); no code path relies on it staying inert. Covered by existing UC2 BDD scenario
  (`use_cases.feature`) plus a new physics-level assertion that PV output actually drops under an
  active export limit.
- [Two independent sources (VTN capacity, plan) resolving to one field could mask which one is
  active] → `PlanSlot.pv_used_kw` and `OadrCapacityState.export_limit_kw` both stay independently
  visible (UI, `/sim`, `/plan`); the tick only combines them for the *physical* clamp, not for
  reporting. No new ambiguity beyond "the tighter of two already-visible values won."
- [MIP-gap/Phase-2-indifference degeneracy curtails PV with no real cost benefit] → Confirmed
  during implementation (not hypothetical) and fixed via Decision 2's tie-break; covered by the
  `pv_used_equals_forecast_when_no_export_constraint_binds` regression test.
