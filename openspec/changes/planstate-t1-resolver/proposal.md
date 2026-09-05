## Why

`asset_max_power` (Spec C, `VEN/src/assets/max_power.rs`) needs a *starting*
`AssetState` at an arbitrary future `t1` — "if the plan runs as intended until
`t1`, what state is each asset in." Nothing today returns that. The closest
existing machinery, `simulator::forecast::build_forecast_frames`, already
re-simulates every controllable asset forward along the plan's own setpoint
schedule (via `Asset::simulate_forward`, which internally produces a full
`Trajectory` of `AssetState` points) — but it only extracts a flattened
`AssetForecastPoint{planned_kw, cap_max_import_kw, cap_max_export_kw}` per
slot, discarding the full state each trajectory point already computed. This
change exposes that state instead of re-deriving it a second time.

## What Changes

- A new function (exact shape decided in design.md) that returns the full
  `AssetState` for a given asset at a requested future `t1`, reusing
  `build_forecast_frames`'s existing per-asset `simulate_forward` calls —
  not a second implementation of the same physics.
- `t1` at or before "now" returns the live snapshot state exactly, with no
  simulation involved — the one point where ground truth is available must
  not carry forecast error.
- PV, which has no `simulate_forward`-driven state evolution (its ceiling is
  resolved fresh from weather every call, not from any evolving state), gets
  an explicit, documented answer for "what does PV's state at t1 mean" rather
  than a value that quietly claims more precision than the model supports.

## Capabilities

### New Capabilities
- `planstate-t1-resolver`: resolving each controllable asset's `AssetState`
  at an arbitrary future `t1`, forecasted along the active plan's own
  schedule.

### Modified Capabilities

(none — this is additive; `build_forecast_frames`'s existing return shape and
callers are unchanged)

## Impact

- `VEN/src/simulator/forecast.rs` — new function alongside
  `build_forecast_frames`, reusing its internals.
- No change to `capacity_forecast.rs`/`envelope_forecast.rs` or any other
  consumer — wiring this resolver (and Spec C's `asset_max_power`) into the
  unified capacity/envelope engine is Spec E's job
  (`docs/plans/asset-max-power-forecast-master-plan.md`).
- Notes but does not resolve `docs/reference/TECHNICAL_DEBTS.md`'s R-69
  (battery efficiency model asymmetry between `battery.rs` and
  `battery_milp.rs`, still open in
  `openspec/changes/battery-efficiency-model-reconciliation/`) — adds one
  verification case that will visibly fail if R-69 is still unresolved when
  this change lands, per the master plan's explicit instruction, rather than
  silently inheriting the gap.
