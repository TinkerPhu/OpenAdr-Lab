## Why

The MILP planner treats PV purely as a forecast input baked as a constant into the grid
power-balance equation (`inputs.p_pv_kw[t]` in `solver_phase1.rs`/`solver_phase2.rs`) — it never
evaluates "would curtailing X kW of PV improve the objective this slot." Tracing the runtime path
while scoping this change also surfaced that PV curtailment has **no physical effect at all** in
the live simulator today: `PvInverter.export_limit_kw` (the field `step_inner` actually clamps
against) is never written by any live code path — `OadrCapacityState.export_limit_kw` is set
correctly from VTN `EXPORT_CAPACITY_LIMIT` events, and `dispatcher.rs` computes a clamped `pv`
setpoint intending to enforce it, but `PvInverter::step()` ignores its `setpoint_kw` argument
entirely, so the clamp never reaches the asset. So both the planner-side gap (no decision variable)
and a pre-existing runtime gap (curtailment never applied) need closing together — a decision
variable that computes a curtailment target nothing ever applies would be an inert feature.
This is backlog task 1 of `docs/plans/deviation-scenarios-analysis.md` §2/§7.

## What Changes

- Add a continuous LP decision variable `p_pv_used[t]` (0 ≤ `p_pv_used[t]` ≤ `p_pv_kw[t]`) to
  `GridMilpVars`, replacing the constant `inputs.p_pv_kw[t]` in the power-balance constraint of
  both solver phases. No new objective term — curtailing is free, so the solver only reduces
  `p_pv_used[t]` below the forecast when doing so relieves an existing constraint (export cap,
  soft-violation penalty), which is the correct incentive (it never sacrifices export revenue
  without reason).
- Expose the solved value as `pv_used_kw` on `PlanSlot` (`entities/plan.rs`, `results.rs`),
  alongside the existing `pv_forecast_kw`; the infeasibility fallback path sets
  `pv_used_kw = pv_forecast_kw` (no curtailment), preserving today's behavior when the solve fails.
- **Fix the dead runtime wiring**: `SimState::tick()` gains a resolved
  `pv_export_limit_override: Option<f64>` parameter, applied to `PvInverter.export_limit_kw` every
  tick (mirroring the existing `apply_tick_overrides` pattern used for the heater). The tick
  pipeline (`tasks/sim_tick/`) computes this value each cycle as the more restrictive of (a) the
  live VTN/sim-inject capacity cap (`OadrCapacityState.export_limit_kw`) and (b) the plan's
  `pv_used_kw` for the current slot (converted to the asset's negative-export sign convention).
  This makes VTN-driven curtailment (previously inert) and the new plan-driven curtailment take
  effect through one shared path.
- VEN UI: surface `pv_used_kw` (and the derived curtailment amount) on the plan-facing PV
  visualization, per the project's UI-transparency rule — a planner decision with no UI-visible
  counterpart is an incomplete implementation.

## Capabilities

### New Capabilities
- `pv-export-curtailment`: the planner can choose to curtail PV export within a planning cycle
  when doing so improves the objective, and that decision (together with any VTN-driven export
  cap) is actually applied to the simulated PV asset every tick and visible in the VEN UI.

### Modified Capabilities
(none — `two-phase-milp` governs the two-phase cost/friction solve structure, not the power-balance
inputs this change touches; no requirement text in that spec changes)

## Impact

- **VEN** (Rust): `controller/milp_interactions.rs` (`GridMilpVars` new field),
  `controller/milp_planner/{solver_phase1,solver_phase2}.rs` (variable declaration, balance
  constraint), `controller/milp_planner/results.rs` + `entities/plan.rs` (`pv_used_kw` field),
  `simulator/mod.rs` (`tick()` new parameter, `PvInverter.export_limit_kw` write),
  `controller/dispatcher.rs` (resolve capacity-cap vs. plan-cap), `tasks/sim_tick/{tick,helpers}.rs`
  (thread the resolved value through).
- **VEN UI** (TypeScript/React): `api/types.ts` (`pv_used_kw` on `PlanSlot`), a plan-facing PV
  chart/panel update to show the curtailed amount when present.
- No VTN, BFF, or openleadr-rs changes — `EXPORT_CAPACITY_LIMIT` event handling is unchanged
  upstream; this only fixes/adds what happens with the resulting capacity value downstream.
- **Non-goals**: no new `AssetMilpContext` implementation for PV (it stays a grid-level variable
  next to `p_imp`/`p_exp`, not a per-asset scheduling context — PV has no startup/ramp/mode
  semantics to justify one); no change to the §5 marginal-cost/shadow-price arbiter design (backlog
  tasks 2–3, separate work); no new objective/cost term for curtailment itself.
