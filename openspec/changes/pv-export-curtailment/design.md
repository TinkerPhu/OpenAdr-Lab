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

**2. No objective term for curtailment.**
Because `p_exp` (export) and `p_imp` (import) already carry the real tariff/GHG/violation costs,
and curtailing PV only ever *reduces* available supply (forcing `p_imp` up or `p_exp` down), the
solver has no incentive to curtail unless doing so relieves a binding constraint elsewhere (the
`p_exp <= p_exp_max_phys_kw * (1 - u_grid)` hard cap or the `p_exp_max_cont_kw` soft-violation
penalty). That is exactly the desired behavior — curtailment as a last resort, never a free
choice — so no tie-break or explicit cost term is needed.

Alternative considered: a tiny anti-curtailment tie-break (symmetric to
`SHIFT_TIEBREAK_EUR_PER_SLOT`) to guard against degenerate alternate optima. Rejected for now —
unlike shiftable-load start slots (many cost-equal starts under flat tariffs), `p_pv_used` only
has one direction of degeneracy (unconstrained slots where curtailing is strictly cost-neutral vs.
not), and HiGHS returns the LP vertex it finds first; if this proves to matter in practice (see
Open Questions) it can be added later without touching the constraint structure.

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
- [LP degeneracy: solver may curtail more than strictly necessary on ties] → Accepted per Decision
  2; revisit with a tie-break only if a real deviation-scenarios experiment shows it matters (see
  Open Questions).

## Open Questions

- Whether LP degeneracy on unconstrained slots ever produces a visibly wrong `pv_used_kw` in
  practice (§ Decision 2) is unknown until exercised — not blocking, addressed by the mitigation
  above if it surfaces.
