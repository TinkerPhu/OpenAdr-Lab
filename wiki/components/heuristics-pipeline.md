---
title: Heuristics Pipeline (learned baselines)
type: component
created: 2026-07-16
updated: 2026-08-21
synced_commit: 35f7808
sources: [VEN/src/services/heuristics.rs, VEN/src/tasks/heuristics_job/mod.rs, VEN/src/entities/design_vocabulary.rs, VEN/src/services/forecast.rs, VEN/src/controller/milp_planner/inputs.rs, VEN/src/routes/debug.rs, VEN/src/assets/base_load.rs, VEN/src/simulator/mod.rs, VEN/src/tasks/history_sampler/mod.rs, VEN/src/controller/report_intervals.rs, VEN/src/controller/reporter.rs, docs/architecture/real_measurement_mqtt.md, docs/history/project_journal.md]
tags: [heuristics, forecasting, baseline, phase-5, wp5-4, baseline-reports]
---

# Heuristics Pipeline (learned baselines)

Phase 5 (WP5.1 + WP5.2, BL-08/BL-14): the VEN learns per-asset behavioral
heuristics from its own persisted history ([[history-store]]) and feeds them to
the [[milp-planner]] as per-slot baseline forecasts, replacing flat scalars.

## The signal: base load (`assets/base_load.rs`)

`base_load` is the site's unmetered background consumption — standby draw,
lighting, plugged-in appliances, anything not modelled as its own asset. It is
the *only* asset this pipeline learns for, and it is genuinely unsimulatable:
its driver is occupant behaviour, which no physics model predicts. Learning the
statistical regularity from history is therefore the correct permanent tool,
not a placeholder (see
[forecasting_model.md](../../docs/architecture/forecasting_model.md)).

Live precedence in `simulator/mod.rs`: a fresh measured MQTT reading wins, else
the site's own learned heuristic (BL-40/R-60), else the synthetic
profile+appliance-noise model as a cold-start last resort.

> A second virtual asset, `site-residual`
> (`grid meter − Σ modelled asset power`), fed this pipeline until 2026-08-21.
> It was removed: in a real deployment `base_load` is itself derived externally
> as `grid_true − Σ(other asset measurements)`, which forces the residual to
> zero by construction. See `docs/architecture/forecasting_model.md` §5.

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
  preload doesn't wait a day). Eligible asset: `base_load`; PV forecasting is
  WP5.3's job, not this pipeline's.
- `POST /debug/heuristics/preload` (`routes/debug.rs`) — generates a synthetic
  4-week backfill and learns from it immediately. The backfill generator
  (`generate_synthetic_backfill`) is shared between this route and the module's
  own tests, so the demo path and the test assertions can never silently
  diverge into two algorithms.

## The consumers

- **Planner** ([[milp-planner]], `inputs.rs`): when a `base_load` heuristic
  exists, each plan slot samples
  `daytime_profile_kw[bucket][hour] × seasonal_factor` instead of repeating a
  flat scalar across the horizon; without one, the pre-heuristic flat behavior
  (`baseline_kw` from the profile) is the fallback.
- **Forecast timeline** (`services/forecast.rs::build_heuristic_forecasts`):
  the same sampling feeds the Controller tab's future-horizon lines in
  [[ven-ui]], which show real daily structure (coffee/lunch/dinner peaks,
  weekend brunch shift) once history is seeded.
- **BASELINE reports** (WP5.4, `controller/report_intervals.rs::build_baseline_report_intervals`):
  a VTN report obligation requesting `payloadType: "BASELINE"` gets each interval's value
  from `AssetHeuristics::sample_kw` summed across assets — the counterfactual "what if this
  event weren't happening" value, submitted alongside the concurrent `USAGE` measurement
  report. Deliberately reuses the *same* event-blind sampling the planner and forecast
  timeline already use rather than a fresh model: the heuristic never saw the event, so it
  needs no adjustment to serve as the counterfactual. Each BASELINE interval also carries a
  `DATA_QUALITY` payload tagged `"HEURISTIC"` — provenance, not a computed statistical
  confidence (`AssetHeuristics` has no sample-count/variance fields to compute one from; a
  real confidence model is a deliberate non-goal, tracked as a future BACKLOG item if ever
  adopted). Downstream, `experiments/kpi.py`'s `event_impact_kwh` diffs archived BASELINE
  vs. USAGE reports per VEN to quantify one event's actual impact in kWh — the piece that
  upgrades SG-3 (report-usefulness evaluation, see [[experiment-harness]]) from directional
  to M&V-grade.

Verified end-to-end on Node1: ven-1's learned weekday bucket shows coffee
(h8), lunch (h12) and dinner (h17–18) peaks while its weekend bucket drops the
lunch peak, adds a brunch peak (h10) and moves dinner an hour earlier — with a
planner integration test proving `baseline_kw` differs for a Saturday-dated vs
Tuesday-dated solve at the same hour ([[testing-strategy]]).

## Composes with [[real-measurement-mqtt]] (found 2026-08-04)

This pipeline doesn't distinguish where a `tick_samples` row's `power_kw` value came from —
`tasks/history_sampler` records whatever `entry.last_power_kw` was that tick, whether it was
the synthetic `appliance_noise_kw` model or a real, live-measured reading. Once
[[real-measurement-mqtt]]'s `base_load_enabled` gate substitutes a measured value into the
live tick, that value flows into `tick_samples` and therefore into this learner with zero
additional plumbing — the two features were built independently but compose automatically.
Verified on ven-1: `GET /forecast`'s `base_load` entry already reports
`"source":"HEURISTIC"` with the learned profile actively driving the planner's forecast.
See [[real-measurement-mqtt]]'s "Indirect path into the forecast" section for the
convergence timeline (EWMA half-life ~14 days, full window 42 days) and the caveat that a
feed dropout silently re-mixes synthetic samples back in with no provenance tag to detect it.

## Distinct from forecast-accuracy tracking

This pipeline *produces* one of the forecasts ([[milp-planner]]'s base_load
input); it does not measure how good that forecast turned out to be. That's a separate,
newer capability — persisted near/far predicted-vs-actual samples for PV and base_load,
reconciled once each prediction's target time elapses — described in
[[history-store]]'s "Forecast accuracy tracking" section and
`docs/architecture/VEN_ARCHITECTURE.md` §4.9a. The two compose: forecast accuracy is exactly
the tool that would let a future change verify whether this pipeline's learned profile is
actually converging, rather than relying on the spot-checks in the "Verified end-to-end"
paragraph above.
