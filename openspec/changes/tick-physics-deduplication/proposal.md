# Proposal: Deduplicate Live-Tick Physics (PV Irradiance, Base Load)

## Why

Found during the same architectural audit as
`battery-efficiency-model-reconciliation` (2026-09-03). Three independently
maintained copies of PV/base-load physics exist:

1. `entities::solar::natural_irradiance_at` (`entities/solar.rs:304-312`) —
   the canonical, shared irradiance-curve formula. Its own doc comment
   ("`assets::PvInverter` (infra) and `controller::milp_planner::inputs`
   (application) previously each kept their own mirrored implementation of
   this identical formula") confirms a prior consolidation effort already
   unified those two call sites onto it. Today, `pv.rs`'s
   `PvInverter::natural_irradiance_at` is a thin one-line wrapper around it,
   `pv_preview.rs` correctly calls that wrapper, and `simulator/forecast.rs`
   correctly reaches it indirectly via `entities::solar::pv_ceiling_kw` (which
   calls it internally, `entities/solar.rs:359`) — none of the three call it
   *directly*, but all three correctly route to it. `tick()` (below) is the
   one holdout that doesn't route to it at all, direct or indirect.
2. `pv_preview.rs::peek_pv_kw` / `base_load_preview.rs::peek_base_load_kw` —
   explicitly documented as manual re-implementations of `SimState::tick()`'s
   own formula ("must stay in lockstep with `tick()`'s formula",
   `pv_preview.rs:18-20`, `base_load_preview.rs:19-22`), protected only by one
   hand-written equivalence test each
   (`peek_pv_kw_matches_tick_output_for_same_now`,
   `peek_base_load_kw_matches_tick_output_for_same_now`) rather than by
   sharing the actual code path.
3. **`SimState::tick()` itself** (`VEN/src/simulator/mod.rs:208-213`) — a
   *third*, independent inline copy of the irradiance formula
   (`sin(π(hour-6)/12)` clamped to 6–18h), which does **not** call
   `entities::solar::natural_irradiance_at` (item 1) despite computing the
   same curve. `tick()` is the one call site that never got consolidated onto
   the shared function, despite being the actual ground-truth simulation, not
   a preview or forecast. `tick()`'s copy is behaviorally a no-op divergence
   today — within the 6–18h domain `angle` ranges `[0, π]`, where `sin` is
   always `≥ 0`, so the canonical function's `.max(0.0)` clamp (absent from
   `tick()`'s copy) never actually changes the result — but it computes `hour`
   via a wasteful and slightly fragile string round-trip
   (`now.format("%H").to_string().parse::<f64>().unwrap_or(12.0)`) instead of
   the canonical function's direct `ts.hour()`/`ts.minute()` field access, so
   consolidating also removes that.

This is the same risk class already tracked for
`capacity_forecast.rs`/`envelope_forecast.rs` (two independently-computed
things that must silently agree) — a second instance of it, in the live-tick
path.

## What Changes

- `SimState::tick()`'s inline irradiance formula is replaced with a call to
  `entities::solar::natural_irradiance_at`, removing copy #3.
- `pv_preview.rs::peek_pv_kw` / `base_load_preview.rs::peek_base_load_kw` are
  changed to call the same underlying logic `tick()` uses (extracted into a
  shared function both `tick()` and the preview functions call), removing the
  "must stay in lockstep by hand" duplication — not just re-pointing at
  `natural_irradiance_at` for the irradiance curve itself, but also
  consolidating the override/smoothing/EMA-decay logic layered on top of it
  that the preview functions currently hand-copy from `tick()`.
- The existing equivalence tests
  (`peek_pv_kw_matches_tick_output_for_same_now`,
  `peek_base_load_kw_matches_tick_output_for_same_now`) are kept — once the
  code is actually shared rather than hand-copied, they become regression
  guards against a *future* accidental fork, rather than the only thing
  currently preventing today's fork from drifting further.

## Non-Goals

- Not a change to what the irradiance/base-load curves compute — same
  formula, same outputs, just one implementation instead of three.
- Not bundled into `openspec/changes/asset-dispatch-trait-objects/`, but
  **sequenced before it** — see "Sequencing" below.

## Capabilities

No capability added/modified. Internal deduplication only — no `specs/` delta.

## Impact

- `VEN/src/simulator/mod.rs` (`tick()`, lines ~205-276)
- `VEN/src/simulator/pv_preview.rs`, `VEN/src/simulator/base_load_preview.rs`
- `VEN/src/entities/solar.rs` (`natural_irradiance_at` — no change expected,
  just an added call site)

## Sequencing — this is preparation work for `asset-dispatch-trait-objects`, not independent follow-up

`asset-dispatch-trait-objects`'s Decision D5 (added 2026-09-03) rewrites
`SimState::tick()`'s hand-written `match cfg { AssetConfig::Pv(pv) => ...,
... }` — the exact same function, the exact same lines this change touches —
into a new `TickOverridable` capability trait implemented by
Pv/Heater/BaseLoad/Ev. If this deduplication happens *after* that migration,
the three-way duplication simply moves inside the new trait methods, fully
baked into the new architecture. Doing it *first* means D5's `TickOverridable`
implementations are built on already-consolidated physics from day one.
**Recommend landing this change before starting
`asset-dispatch-trait-objects`'s Phase 2a tasks for PV and BaseLoad
(tasks.md 4.4/4.5).**
