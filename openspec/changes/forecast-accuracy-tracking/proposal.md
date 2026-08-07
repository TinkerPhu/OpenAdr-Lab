## Why

Two forecasts drive planning today — PV's weather/physics-based `pv_forecast_kw` and base_load/
site-residual's learned-heuristic sample — but neither is ever persisted. Once a slot elapses,
there's no way to tell whether the forecast that drove that plan was any good, so forecast quality
can only be argued from first principles, never checked against reality (`docs/plans/
forecast-accuracy-idea.md`, parked 2026-08-04).

That parked idea left two questions open: what fixed grid to key persisted forecasts on (plan
slot boundaries shift every replan, so they can't be a stable join key across cycles), and whether
base_load should get equivalent tracking to PV or ship PV-only first. Both are resolved here: no
fixed grid at all — track only the plan's nearest and farthest slot each cycle — and base_load
(plus site-residual) ship together with PV from the start.

## What Changes

- Every plan cycle, immediately after a plan resolves (`finish_plan_cycle`), record two forecast
  samples per forecastable asset (PV, base_load, site-residual — the same three assets that already
  get a `ForecastSource::WeatherModel`/`Heuristic`-tagged entry in the live `/forecast` API, never
  dispatchable/`Optimization`-sourced assets):
  - **near**: the second slot's (`plan.slots[1]`) forecast value and target time — the closest
    genuinely-future instant, deliberately skipping `plan.slots[0]` because it starts at `now` and
    is what's currently being commanded, not a forecast about to be tested.
  - **far**: the last slot's (`plan.slots.last()`) forecast value and target time — the
    longest-lead prediction the current horizon reaches.
  - PV's value is read straight from the existing `PlanTimeSlot.pv_forecast_kw` (no new field). Base
    load and site-residual are sampled directly from `AssetHeuristics::sample_kw(target_ts)` (no new
    `Plan`/`PlanTimeSlot` field either) — the same call `build_heuristic_forecasts` already makes for
    the live forecast API.
  - Written unconditionally every cycle (no value-change gating) — at `replan_interval_s`'s default
    (300s), this produces one new row per asset per point roughly every 5 minutes.
- New small table, `forecast_accuracy_samples`, via the existing `HistoryPort`: `(asset_id,
  lead_kind, target_ts, predicted_kw, predicted_at, actual_kw, actual_at)`. `actual_kw`/`actual_at`
  start `NULL` and are filled in later.
- Reconciliation piggybacks on `history_sampler`'s existing 1-minute downsample flush
  (`tasks/history_sampler/mod.rs::write_window`): whenever a flushed `tick_samples` window lands for
  an asset, any still-open forecast row for that asset whose `target_ts` falls inside the flushed
  window gets its `actual_kw`/`actual_at` filled in from that window's mean. No separate
  reconciliation task.
- `GET /history/forecast-accuracy?from=&to=&asset_id=&lead_kind=`, mirroring the existing
  `history_range_route!` pattern in `routes/hems/history.rs`.
- VEN UI: overlay the reconciled near/far series onto the existing History page's per-asset
  `AssetTimelineChart` for PV, base_load, and site-residual, alongside the actual line already
  rendered there.

## Capabilities

### New Capabilities
- `forecast-accuracy-tracking`: the VEN records, per plan cycle, its closest- and farthest-lead
  forecast for PV, base_load, and site-residual power; reconciles each recorded forecast with the
  real measured/simulated value once that instant elapses; and exposes both series for query and
  UI display alongside the actual value.

### Modified Capabilities
(none — additive)

## Impact

- **VEN** (Rust): `entities/history.rs` (`ForecastAccuracySample`, `ForecastLeadKind`),
  `controller/history_port.rs` (`HistoryPort::append_forecast_samples` /
  `reconcile_forecast_actuals` / `query_forecast_accuracy`), `history_store/schema.rs` (schema v8,
  new table), `history_store/mod.rs` (implementation + `prune_before` extension),
  `services/forecast.rs` (`finish_plan_cycle` gains the near/far capture step),
  `tasks/history_sampler/mod.rs` (`write_window` gains the reconciliation call),
  `routes/hems/history.rs` (+ `routes/mod.rs` wiring) for the new query route.
- **VEN UI**: `components/controller/charts/AssetTimelineChart.tsx` (near/far overlay series),
  wherever it's fed for the PV/base_load/site-residual cells on the History page.
- **Non-goals**: no fixed canonical grid across the full horizon (superseded by the near/far-only
  design); no tracking for dispatchable/controllable assets (battery, EV, heater) — their planned
  value is a command being executed, not an external prediction to validate; no revival of
  `plan_snapshots` (dropped in R-63) or any plan-replay mechanism.
- No VTN, BFF, or openleadr-rs changes.
