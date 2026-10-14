# Proposal

## Why

W1 (`asset-declared-planner-model`, merged 2026-10-14) made the planner core plan whatever each
asset declares. Most of W2 went with it: allocations, the PV-surplus claim order, planned state,
session flexibility and the EV's firm-shortfall diagnostic all come from the asset now. What is
left of W2 is the plan's wire format, which still names one battery:

- `PlanTimeSlot.bat_charge_kw` / `bat_discharge_kw` and `Plan.soc_trajectory_kwh` are the summed
  storage legs. Nothing reads them: no VEN module, no UI component, no BFF, no BDD step (only
  tests, fixtures and the generated TypeScript types). Each storage asset's own facts already go
  out per asset, as its net power (`allocations`, `planned_kw_by_asset`) and its planned state of
  charge (`planned_state_by_asset`). The summed fields are a second copy no one consumes, and
  `audit_ven_architecture.py` rule 16 has to exempt them by name.
- The plan does not say which assets it planned. An asset the plan keeps idle has no allocation, so
  the Planner tab's Decision Matrix (`PlanDecisionMatrix.tsx`) guesses its rows from the
  allocations, hard-codes a `"battery"` row so an idle battery stays visible, and filters out
  `pv` and `base_load` by name. An idle heater, EV or shiftable load has no row at all.

## What Changes

- **BREAKING (wire, no known reader):** delete `PlanTimeSlot.bat_charge_kw`,
  `PlanTimeSlot.bat_discharge_kw` and `Plan.soc_trajectory_kwh`. Plans already stored as JSON
  (`plan_snapshots.plan_json`, the persisted active plan) still load: unknown fields are ignored.
- Add `Plan.planned_asset_ids`: the roster ids of the assets this plan scheduled, in planner input
  order, idle ones included, on solved and fallback plans alike (`#[serde(default)]`, so older
  stored plans read as an empty list).
- The Decision Matrix shows one row per planned asset (plus any asset an older plan allocates) and
  loses its `"battery"`, `"pv"` and `"base_load"` literals.
- Dispatch is unchanged: an asset without an allocation in a slot still runs at its own default
  setpoint or thermostat (`dispatcher::build_setpoints`). `planned_asset_ids` is plan output for
  readers, not a new dispatch rule.
- `audit_ven_architecture.py` rule 16 drops its plan-wire exemption and also checks
  `entities/plan.rs`, so the plan's wire types cannot gain a kind-named field again.

## Capabilities

### New Capabilities
- `plan-asset-output`: what the plan says per asset on the wire: which assets it planned, and that
  every per-asset fact is keyed by asset id with no kind-named site field.

### Modified Capabilities
<!-- none: no capability spec under openspec/specs/ covers the plan wire format -->

## Impact

- **VEN Rust:** `entities/plan.rs`, `controller/milp_planner/results.rs`, about 15 hand-built
  `Plan` / `PlanTimeSlot` literals in tests and fixtures (`dispatcher.rs`, `reporter.rs`,
  `timeline.rs`, `routes/timeline.rs`, `services/forecast.rs`, `simulator/forecast.rs`,
  `services/test_support/plans.rs`, `tasks/sim_tick/tick_tests.rs`, `controller/tests`), the
  planner tests that read the deleted fields, and `tests/golden/plans.txt`.
- **UI:** regenerated `api/generated/Plan.ts` and `PlanTimeSlot.ts`, `__tests__/fixtures/plan.ts`,
  `components/planner/PlanDecisionMatrix.tsx` and its test.
- **BDD:** one scenario in `tests/features/ven_planner.feature`.
- **Scripts:** `scripts/audit_ven_architecture.py` rule 16 and its test.
- **Not affected:** the model, every solve, `model_fingerprints.txt` and `milp_inputs.txt`; the
  BFF and VTN (they do not read the plan's slot fields); dispatch.
- **Out of scope:** PV and base load as planner participants (W3: `baseline_kw`, `pv_forecast_kw`,
  `pv_used_kw` stay site series), sessions per asset (W4), fixed-id lookups outside the planner
  (W5), the UI's other per-kind code such as `PlanPowerStack.tsx`'s render order (W6).
