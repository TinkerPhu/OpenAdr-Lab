## Why

`pv-export-curtailment` made PV curtailment real (a MILP decision, and a runtime path that
applies it), but curtailment itself is still invisible after the fact — the Controller page's PV
graph shows no distinction between a normal, an inverter-hardware-capped, and a genuinely curtailed
period, past or future, and nothing persists the applied limit for later inspection.

An earlier draft of this change proposed storing a simulator-only "potential vs. actual" delta.
That was rejected during scoping: a real inverter under a curtailment command doesn't report what
it *would have* produced — only the limit it was given and its actual output — so persisting a
delta as ground truth wouldn't generalize past the simulator. Scoping also surfaced a real model
gap: PV profiles only carry `rated_kw` (installed **DC panel peak**); there is no separate
**inverter AC output capability**, so a benign, hardware-side ceiling (common when a system is
deliberately DC/AC-oversized) cannot today be told apart from a real, externally-imposed limit —
and conflating the two would make the very distinction this change exists to draw actively wrong.

## What Changes

- Add `inverter_max_kw` to the PV profile (`PvConfig`/`PvParams`), defaulting to `rated_kw` when
  unset — every existing profile is unaffected unless someone explicitly configures a lower value.
- `PvInverter::step_inner` (and the forecast/MILP-input functions that compute DC potential —
  `forecast_kw_at`, `capability_trajectory`, `build_milp_context`) clamp DC potential to
  `inverter_max_kw` *before* any commanded `export_limit_kw` is applied, modeling the inverter's own
  AC-side clipping as a distinct, always-present ceiling. With the default
  (`inverter_max_kw == rated_kw`) this is a no-op.
- Move `export_limit_kw` from `PvInverter` (config/self) onto `PvState` (per-tick state), fixing a
  latent inaccuracy: historical points reconstructed from the in-memory `AssetHistoryBuffer`
  currently read the *live* `self.export_limit_kw`, not what was actually active at that past
  tick.
- Tag, at the moment `resolve_pv_export_limit_kw` resolves the effective limit, which source
  determined it — the plan's own target, or a live VTN/capacity cap the plan didn't anticipate —
  and carry that tag onto `PvState` alongside the limit value. No plan-snapshot persistence or
  retrospective cross-reference is needed for this: the distinction is knowable synchronously, at
  the moment the limit is resolved.
- Persist `export_limit_kw` and the source tag into long-term history (`tick_samples`, two small
  nullable columns, schema v5) via the existing accumulator/`state_values()` mechanism — the same
  path `soc_pct`/`temperature_c` already use, so no new plumbing layer.
- VEN UI: `AssetTimelineChart.tsx` (Controller page PV graph) shades three distinguishable states —
  none, hardware-capped (neutral, informational), and imposed curtailment (amber = planned, red =
  unplanned, past only) — reusing its existing `ReferenceArea`/`zones` mechanism.
- Fix `controller/timeline.rs`'s PV branch to plot `-slot.pv_used_kw` for future points instead of
  `-slot.pv_forecast_kw` — a sibling of the fix already applied to `PlanPowerStack.tsx` in
  `pv-export-curtailment`, missed for this chart.

## Capabilities

### New Capabilities
- `pv-curtailment-history`: PV profiles declare the inverter's true AC output capability
  separately from installed panel peak; the VEN distinguishes hardware-side clipping from
  externally-imposed curtailment, tags imposed curtailment as planned or unplanned at the moment
  it's resolved, persists the applied limit and its source in long-term history, and renders all
  three states as visually distinct bands on the Controller page's PV timeline.

### Modified Capabilities
(none — additive)

## Impact

- **VEN** (Rust): `profile/schema.rs` + `entities/asset_params.rs` (`inverter_max_kw` field),
  `assets/pv.rs` (`PvState` gains `export_limit_kw`/`curtailment_source`; `step_inner` and the
  forecast functions clamp to `inverter_max_kw`; `state_values()` reads from `state`, not `self`),
  `controller/dispatcher.rs` (`resolve_pv_export_limit_kw` returns the resolved value *and* its
  source), `entities/history.rs` + `history_store/{schema,mod}.rs` (schema v5, two new columns),
  `tasks/history_sampler/accumulator.rs` (two more accumulated fields), `controller/timeline.rs`
  (PV branch `pv_used_kw` fix).
- **VEN UI**: `components/controller/charts/AssetTimelineChart.tsx` (three-state shading),
  `components/controller/AssetMidSection.tsx` (pass through for the PV cell).
- **Non-goals**: no plan-snapshot persistence or retrospective classification (superseded by the
  live source tag); no change to `pv-export-curtailment`'s decision variable or tie-break; no
  profile validation rule tying `inverter_max_kw` to `rated_kw` beyond "> 0" (a system can be
  legitimately over- or under-sized either way).
- No VTN, BFF, or openleadr-rs changes.
