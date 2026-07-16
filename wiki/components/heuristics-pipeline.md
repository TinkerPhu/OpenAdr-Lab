---
title: Heuristics Pipeline (learned baselines)
type: component
created: 2026-07-16
updated: 2026-07-16
synced_commit: f08e469
sources: [VEN/src/services/heuristics.rs, VEN/src/tasks/heuristics_job/mod.rs, VEN/src/controller/residual.rs, VEN/src/entities/design_vocabulary.rs, VEN/src/services/forecast.rs, VEN/src/controller/milp_planner/inputs.rs, VEN/src/routes/debug.rs, VEN/src/assets/base_load.rs, docs/history/project_journal.md]
tags: [heuristics, forecasting, baseline, phase-5]
---

# Heuristics Pipeline (learned baselines)

Phase 5 (WP5.1 + WP5.2, BL-08/BL-14): the VEN learns per-asset behavioral
heuristics from its own persisted history ([[history-store]]) and feeds them to
the [[milp-planner]] as per-slot baseline forecasts, replacing flat scalars.

## The signal: SITE_RESIDUAL (`controller/residual.rs`)

`residual_kw = grid meter (kW) − Σ modelled asset power (kW)` — unmodelled site
consumption, exposed as a read-only virtual asset (`site-residual`, zero
import/export capability so it can never be dispatched). Not clamped: a negative
residual (modelled assets exceeding the meter) is a signal worth surfacing.
Computed against the raw snapshot *before* synthetic assets (shiftable-load
runtimes) are inserted, so a running shiftable load isn't misread as
"unexplained" load. Inserted at both consumers of the 1 s snapshot — the tick
publish path and the history sampler's own independent snapshot — so its
history accumulates in `tick_samples`.

In pure simulation the residual is structurally 0: the simulator *derives* the
grid meter as the sum of its modelled assets, so the two terms can never
disagree (recorded as R-20; the mechanism exists for when a real meter feed
arrives).

## The phenomenon: configured appliance noise (`assets/base_load.rs`)

BaseLoad supports a `base_load.spikes` list in the profile
(`profile::schema::SpikeConfig`, empty by default): each spike is a
**trapezoidal** daily pulse — flat plateau at `amplitude_kw`, linear ramps,
day-to-day jitter in timing and magnitude, optional weekday restriction, and a
per-day firing `probability`. A trapezoid rather than a Gaussian because its
energy is directly `≈ amplitude_kw × (duration_h − ramp_h)`, settable to match
a real appliance session; Gaussian tails make the integral uncontrollable. This
gives the learner a realistic, non-flat signal to recover.

## The learner: `services/heuristics.rs` (application ring)

`learn_asset_heuristics(&dyn HistoryPort, asset_id, now, cfg)` is a pure
aggregation: two independent EWMA-recency-weighted mean-power-by-hour-of-day
passes — one fed by weekday ticks, one by weekend ticks — plus a rolling
seasonal factor. Defaults: 42-day window, 14-day EWMA half-life, and a
cold-start gate (`min_samples_for_confidence`, 100 ticks) below which it
returns `Ok(None)` and the flat fallback stays in place rather than fitting
noise.

The result is `AssetHeuristics` (`entities/design_vocabulary.rs`):
`daytime_profile_kw: [Vec<f64>; 2]` (`[0]`=weekday Mon–Fri, `[1]`=weekend) ×
`seasonal_factor`, sampled via `sample_kw(slot_t)` which picks the bucket from
`slot_t.weekday()`. Two buckets, not seven: a 28-day seeding window gives each
weekend bucket ~8 days of samples (stable mean) but would starve a 7-way split
to ~4 samples per weekday (limit recorded in TECHNICAL_DEBTS.md).

## Scheduling and seeding

- `tasks/heuristics_job/` — daily background job (mirrors
  `history_sampler`'s day-boundary shape, fires on first check too so a fresh
  preload doesn't wait a day). Eligible assets: `base_load` and
  `site-residual`; PV forecasting is WP5.3's job, not this pipeline's.
- `POST /debug/heuristics/preload` (`routes/debug.rs`) — generates a synthetic
  4-week backfill and learns from it immediately. The backfill generator
  (`generate_synthetic_backfill`) is shared between this route and the module's
  own tests, so the demo path and the test assertions can never silently
  diverge into two algorithms.

## The consumers

- **Planner** ([[milp-planner]], `inputs.rs`): when a heuristic exists for
  `base_load`/`site-residual`, each plan slot samples
  `daytime_profile_kw[bucket][hour] × seasonal_factor` instead of repeating a
  flat scalar across the horizon; without one, the pre-heuristic flat behavior
  (`baseline_kw` from the profile, live residual reading) is the fallback.
- **Forecast timeline** (`services/forecast.rs::build_heuristic_forecasts`):
  the same sampling feeds the Controller tab's future-horizon lines in
  [[ven-ui]], which show real daily structure (coffee/lunch/dinner peaks,
  weekend brunch shift) once history is seeded.

Verified end-to-end on Pi4: ven-1's learned weekday bucket shows coffee
(h8), lunch (h12) and dinner (h17–18) peaks while its weekend bucket drops the
lunch peak, adds a brunch peak (h10) and moves dinner an hour earlier — with a
planner integration test proving `baseline_kw` differs for a Saturday-dated vs
Tuesday-dated solve at the same hour ([[testing-strategy]]).
