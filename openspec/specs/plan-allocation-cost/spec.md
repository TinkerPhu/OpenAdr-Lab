# plan-allocation-cost Specification

## Purpose
Defines the sign convention for `AssetAllocation.cost_eur` (per-slot, per-asset cost shown on the
Planner tab's decision matrix), so it stays consistent with `FlexibilityEnvelope`'s
session-total cost estimate — both must price energy covered by PV surplus the same way.

## Requirements

### Requirement: PV-surplus consumption priced as opportunity cost
`AssetAllocation.cost_eur`, computed per slot per asset in
`controller/milp_planner/results.rs::translate_to_plan` for the EV, heater, shiftable-load, and
battery-charging allocation blocks, SHALL price energy covered by PV surplus as forgone export
revenue (an opportunity cost added to the slot's cost), not as a credit subtracted from it:

```
cost_eur = grid_power_kw * import_tariff_eur_kwh * dt_h
         + surplus_power_kw * export_tariff_eur_kwh * dt_h
```

This matches the convention already used by
`controller/milp_planner/envelopes.rs::solved_session_cost()`.

#### Scenario: Slot fully covered by PV surplus reports forgone-export cost
- **WHEN** an asset allocation block computes `cost_eur` for a slot where `grid_power_kw` is 0 and
  `surplus_power_kw` is fully covering the asset's demand
- **THEN** `cost_eur` equals `surplus_power_kw * export_tariff_eur_kwh * dt_h` (positive), not its
  negative

#### Scenario: Mixed grid-import and PV-surplus slot sums both terms
- **WHEN** an asset allocation block computes `cost_eur` for a slot with nonzero `grid_power_kw`
  and nonzero `surplus_power_kw`
- **THEN** `cost_eur` equals `grid_power_kw * import_tariff_eur_kwh * dt_h + surplus_power_kw *
  export_tariff_eur_kwh * dt_h`

#### Scenario: Decision matrix and envelope session total agree in sign
- **WHEN** a plan spans multiple asset types (EV, heater, shiftable-load, battery-charging) with
  PV-surplus-covered slots for each
- **THEN** the sum of `AssetAllocation.cost_eur` across the decision matrix and
  `FlexibilityEnvelope.estimated_cost_eur` (from `solved_session_cost()`) agree in sign for the
  same slot range
