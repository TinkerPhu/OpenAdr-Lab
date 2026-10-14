# Design

## Context

W1 left the plan's assembly asset-declared (`results.rs::translate_to_plan` loops over each
asset's `AssetSolution`), but the plan's wire types still carry three battery-named fields, filled
from `results.rs::site_storage`, and the plan does not say which assets it scheduled. This change
finishes the result side of W2. The original W2 estimate (3–5 days) assumed the battery fields had
readers that would need a per-asset replacement; a search found none, so the work is a deletion
plus one new field.

## Inventory: where the concepts live today

**"The site's storage legs" (charge, discharge, stored energy):**

| Where | What it does | Becomes |
|---|---|---|
| `controller/milp_planner/results.rs::site_storage` | Sums every `AssetSolution.storage` into one `StorageLegs` | Kept for the cost breakdown's wear figure only, renamed `storage_throughput_kw` and returning one series (charge + discharge per slot); its `energy_kwh` sum goes |
| `results.rs::translate_to_plan` | Writes `bat_charge_kw`, `bat_discharge_kw` per slot and `soc_trajectory_kwh` | Deleted |
| `results.rs::fallback_plan` | Writes zeros for the same fields | Deleted |
| `entities/plan.rs` | Declares the three fields | Deleted |
| Readers | None in production. Tests: `tests/planner.rs` (power balance, battery absent, cheap-charge), `tests/heater.rs:489`, `tests/multi_instance.rs` (sum of two batteries). UI: generated types and `__tests__/fixtures/plan.ts` only | Tests read the battery's own `planned_kw_by_asset` / allocation and its `AssetSolution.storage`; the multi-instance test checks each battery's own legs |

**"Which assets did this plan schedule":**

| Where | How it answers today | Becomes |
|---|---|---|
| `VEN/ui/src/components/planner/PlanDecisionMatrix.tsx` | Ids from allocations, minus `"pv"`/`"base_load"`, plus a hard-coded `"battery"` | `plan.planned_asset_ids` ∪ allocation ids, no literals |
| `services/forecast.rs::build_asset_forecasts` | Ids from every slot's `planned_kw_by_asset` and `planned_state_by_asset` keys | `Plan::planned_assets()` |
| `controller/timeline.rs` (asset timeline) | Is asked for one id by its caller; does not enumerate | Unchanged |
| `results.rs` | Knows the answer (`asset_contexts`) and drops it | Writes `Plan.planned_asset_ids` |

**Hand-built `Plan` / `PlanTimeSlot` literals** that name the deleted fields (all test or
fixture code): `controller/dispatcher.rs`, `controller/reporter.rs`, `controller/timeline.rs`,
`controller/tests/arbiter_tests.rs`, `routes/timeline.rs`, `services/forecast.rs`,
`services/planning/mod.rs` (a JSON literal), `services/test_support/plans.rs`,
`simulator/forecast.rs`, `tasks/sim_tick/tick_tests.rs`.

## Decisions

**D1. Delete the battery fields; do not rename them to kind-neutral site totals.** A
`storage_charge_kw` total would be a third copy of what each storage asset already reports, with no
reader. Per asset, `planned_kw_by_asset[id]` is the battery's net power: its direction selector
(`battery_declared.rs`, R-97) never charges and discharges in one slot, so charge = max(net, 0) and
discharge = max(−net, 0). The state of charge per slot is `planned_state_by_asset[id]["soc"]`.
Alternative rejected: keep the totals "for the UI". The UI has never read them; a future reader
can ask for a per-asset field then, with a consumer in the same change (`no-half-built-features`).

**D2. `Plan.planned_asset_ids: Vec<String>`, in planner input order.** Written by
`translate_to_plan` and `fallback_plan` from `asset_contexts`, the one place that knows. Input
order, not sorted, because that is the order the planner reports everything else in (warnings,
`AssetSolution`s). `#[serde(default)]`: plans in `plan_snapshots` and the persisted active plan
predate it.

**D3. One reader-side answer, `Plan::planned_assets()`.** It returns `planned_asset_ids` followed
by any id an allocation, `planned_kw_by_asset` or `planned_state_by_asset` names that the list
lacks. For a plan with the field the tail is empty; for an older plan it is today's guess. Both
Rust readers call it. The UI cannot call Rust, so `PlanDecisionMatrix.tsx` applies the same rule
(list, then allocation ids) in one `useMemo`, and its test pins the older-plan case.

**D4. Dispatch is not touched.** `dispatcher::build_setpoints` keeps treating "no allocation in this
slot" as "the asset runs at its own default or thermostat". Using `planned_asset_ids` to force a
planned-idle asset to 0 kW would change live behaviour (the heater's thermostat fallback, the EV
opportunistic lever) and is not part of W2.

**D5. `build_asset_forecasts` covers every planned asset.** A planned asset that the plan keeps idle
and that has no planned state (a shiftable load waiting for its window) now gets an
`Optimization` forecast of zeros instead of none, which also stops a heuristic forecast taking its
place. This is what the plan says about it. Pinned by a unit test.

**D6. Audit rule 16 covers the plan's wire types.** `entities/plan.rs` joins the checked paths and
the `PLAN_WIRE_FIELDS` exemption is deleted. Comments are stripped by the audit, and
`WarningKind::EvCoreEnergyUnmet` is CamelCase, so the rule's `bat_|ev_|heat_|heater_` pattern finds
nothing in the current file. `scripts/test_audit_ven_architecture.py` gains a case that plants
`bat_charge_kw` in a copy of `entities/plan.rs`.

## Verification

- `model_fingerprints.txt` and `milp_inputs.txt` unchanged (no `UPDATE_*` variable).
- `plans.txt` regenerated, then checked by a script: the old golden with every
  `"bat_charge_kw":…,"bat_discharge_kw":…,` pair and every `soc_trajectory_kwh` line removed, and
  one `planned_asset_ids` line added per plan, equals the new golden byte for byte.
- Test-first: the new `results.rs` and `Plan::planned_assets` unit tests, the matrix test, the
  forecast test and the BDD scenario fail before the change.
- All four suites on Node2.

## Risks

- A reader outside this repo parsing `bat_charge_kw`. Search found none in the BFF, the VTN,
  `experiments/` or `lab-ui/`; the field was only ever produced.
