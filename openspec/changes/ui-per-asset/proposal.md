# Proposal

## Why

W1–W5 made the VEN plan, control and observe every asset under its own id. The VEN UI still
renders one fixed asset per kind: the Devices page hard-codes `EV_ASSET_ID = "ev"` and a heater
card for `"heater"`, the Controller's builders read `sim.assets["ev"]`, `["pv"]`, … and keep a
`HARDCODED_IDS` list, the tariff builders a fixed `ASSET_IDS` list, the comfort-curve card a fixed
list of three ids, the plan stack a fixed render order. A second battery or EV would be invisible
or mislabelled. This is W6.

## What Changes

- `/sim` carries each asset's roster position (`roster_index`), so the UI lists assets in profile
  order; one shared helper (`assetsOfType`) answers "the assets of this type, in roster order"
  from `asset_type`, which each asset declares.
- Devices: one EV card per EV and one heater card per heater, titled with the asset id, each with
  its own settings, usage view, capability and requests; a card submits requests for its own
  asset (user decision: each card is the asset's picker).
- Controller: one summary cell and one right-hand section per asset, reading that asset's own
  snapshot; tariff/allocation series for every asset of the roster.
- Comfort curves: every asset that takes a request, by its own id.
- Plan stack and stacked charts: series per asset, ordered by asset type, then roster.
- Dashboard: the PV badge for any PV asset.

## Capabilities

### New Capabilities
- `ui-per-asset`: the VEN UI renders every asset of the roster under its own id.

## Impact

- VEN UI (`pages/Devices.tsx`, `pages/Dashboard.tsx`, `components/controller/*`,
  `components/devices/*`, `components/planner/PlanPowerStack.tsx`,
  `components/charts/StackedTimeSeriesChart.tsx`); VEN `controller/sim_snapshot.rs`
  (`roster_index` on the wire).
- One-asset-per-kind sites look as today, the asset id already being the kind's name.
- Out of scope (W7): lifting the profile's one-per-kind validation, multi-instance profiles.
