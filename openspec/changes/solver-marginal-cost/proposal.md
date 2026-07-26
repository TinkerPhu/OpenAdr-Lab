## Why

`docs/plans/deviation-scenarios-analysis.md` §5.2 identifies the missing piece for any future
real-time deviation arbiter: a per-slot **shadow price** on the MILP's power-balance constraint —
"how much the objective would change if the site were forced to import one more kWh right now."
The plan itself is out of scope here (§5.3, backlog item 2, explicitly deferred to its own focused
pass since it's the piece that has failed twice before — see §1). This change is scoped to backlog
item 1 only: get the number out of the solver and make it visible, with no arbiter behavior
attached to it yet.

Raw MILP duals aren't meaningful once integers are involved (branch-and-bound doesn't preserve LP
duality), so the design calls for a second, cheap LP solve per planning cycle: take the winning
MILP's binary assignments, fix them, and re-solve as a pure LP to read a real dual off the
power-balance row. This is additive to `SolveOutput`/`Plan` and does not change any planning
decision — `p_imp`/`p_exp`/allocations are untouched; the new fields are read-only diagnostics.

## What Changes

- Add a second, binaries-fixed LP solve (`solver_duals.rs`) run once per planning cycle after the
  winning two-phase MILP solve. Reuses the same variable/constraint/objective construction as
  Phase 1, fixes every binary variable (`u_grid`, battery `u_bat`, EV `z_ev_on`/`z_ev_core`, heater
  `z_heat_mid`/`z_heat_full`/`z_heat_ready`, shiftable-load `y_shift`) to the winning solution's
  values, and reads the dual of each slot's power-balance constraint via `good_lp`'s
  `SolutionWithDual`/HiGHS backend (already supports this — confirmed via `good_lp`'s
  `shadow_price.rs` test).
- Add `marginal_cost_import_eur_per_kwh` / `marginal_cost_export_eur_per_kwh` to `PlanTimeSlot`
  (`entities/plan.rs`), `#[serde(default)]` for backward-compatible deserialization of persisted
  plans. Per §5.2, both fields carry the *same* single dual value for now — documented explicitly
  as a harmless simplification under cost-minimizing objectives (import/export tariffs are close to
  linear through zero); a future self-consumption-style objective is the trigger for splitting them
  via the lazy opposite-direction resolve §5.2 already describes, not part of this change.
  Add `#[serde(default)]` mirrors the fallback plan and infeasible path.
  This change does **not** rewire `AssetAllocation.marginal_value` (already tariff-based) — that's
  arbiter-consumption territory (backlog item 2), out of scope here.
- Fall back gracefully (not planning-fatal) if the dual LP fails: log a warning and fill both
  fields with the plain import/export tariff for that slot (the existing pre-shadow-price
  approximation), instead of failing the whole plan.
- VEN UI: add a "Marginal cost" row to the Planner "Decision Matrix" (`PlanDecisionMatrix.tsx`),
  reusing the existing tariff heatmap/tooltip pattern, so the shadow price is visible per slot next
  to the tariff it's derived from (satisfies `ui-transparency` — this is exactly the kind of derived
  state the rule requires a surface for).
- No **BREAKING** changes — additive solver step, additive `#[serde(default)]` fields, additive UI
  row.

## Capabilities

### New Capabilities
- `solver-marginal-cost`: the binaries-fixed dual LP solve, the two new `PlanTimeSlot` fields, and
  their VEN UI Decision Matrix row.

### Modified Capabilities
(none — no existing requirement's behavior changes; the MILP's actual decisions are unaffected.)

## Impact

- **Affected service**: VEN only (Rust solver + entity + route pass-through since `Plan` already
  serializes to `GET /plan`) and VEN UI (Decision Matrix row). No VTN/BFF changes.
- **Affected files**: `VEN/src/controller/milp_planner/solver_duals.rs` (new),
  `VEN/src/controller/milp_planner/solver_phase1.rs` (`add_model_constraints` gains
  power-balance `ConstraintReference`s as a return value), `VEN/src/controller/milp_planner/mod.rs`
  and `results.rs` (wiring), `VEN/src/entities/plan.rs` (new fields on `PlanTimeSlot`), the ~8
  existing `PlanTimeSlot { .. }` literal construction sites elsewhere in the codebase (tests/mocks —
  mechanical addition of the two new fields, no behavior change),
  `VEN/ui/src/api/types.ts` + `VEN/ui/src/components/planner/PlanDecisionMatrix.tsx`.
- **Preconditions satisfied**: none required — this is backlog item 1, buildable standalone.
  Item 2 (the arbiter) still requires this change plus the (separately-scoped, higher-risk)
  arbitration work in §5.3.
