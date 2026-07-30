## Why

`AssetAllocation.cost_eur` (the Planner tab's per-slot/per-asset cost display, built in
`results.rs::translate_to_plan`) and `FlexibilityEnvelope.estimated_cost_eur`
(`solved_session_cost()` in `envelopes.rs`, added by the BL-36 rebuild) compute the cost of
PV-surplus-covered energy with opposite signs. `translate_to_plan` still uses
`grid_power_kw × import_tariff − surplus_power_kw × export_tariff` (a credit), while
`solved_session_cost` uses `+` (an opportunity cost: consuming surplus instead of exporting it
forfeits export revenue). The Planner tab's per-slot costs and the envelope's session total can
therefore visibly disagree in sign for the same data — a data-integrity/trust issue, not a
cosmetic one. Ref: `docs/BACKLOG.md` BL-40.

## What Changes

- Flip the sign on the PV-surplus term in all four `AssetAllocation.cost_eur` computations in
  `VEN/src/controller/milp_planner/results.rs::translate_to_plan` (EV, heater, shiftable-load,
  battery-charging blocks), so surplus-covered energy is priced as forgone export revenue
  (`+surplus_power_kw × export_tariff_eur_kwh × dt_h`) instead of a credit, matching
  `solved_session_cost()`'s convention.
- No change to `solved_session_cost()` itself — it is already correct and is the reference
  convention.
- No change to solver objective weights or MILP constraints — this is a display/reporting
  computation only, downstream of the solve.

## Capabilities

### New Capabilities
- `plan-allocation-cost`: defines the sign/semantics convention for `AssetAllocation.cost_eur`
  (per-slot, per-asset allocation cost as reported in the decision matrix / Planner tab) and its
  agreement with `FlexibilityEnvelope.estimated_cost_eur` on PV-surplus-covered energy.

### Modified Capabilities
(none — no existing spec currently documents `AssetAllocation.cost_eur`'s sign convention)

## Impact

- **Affected code**: `VEN/src/controller/milp_planner/results.rs` (`translate_to_plan`, all four
  allocation blocks) and their existing unit tests in `VEN/src/controller/milp_planner/tests/`
  that assert the old sign.
- **Affected containers/services**: VEN only (planner/decision-matrix computation). No VTN, BFF,
  or UI code changes — the UI already renders whatever sign `cost_eur` carries.
- **No OpenADR 3.1 spec constraint** — this is an internal cost-accounting convention, not
  wire-protocol behavior.
- **No openleadr-rs change required.**
- **Non-goals**: does not change `PlanSummary.total_cost_eur` / `CostBreakdown.c_energy_eur`
  (grid-level cost, a separate computation: `net_import × import − net_export × export`); does
  not change solver objective weights; does not add new UI surfaces (existing Planner tab already
  displays `cost_eur` as-is).
