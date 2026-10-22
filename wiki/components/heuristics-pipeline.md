---
title: Heuristics Pipeline (learned baselines)
type: component
created: 2026-07-16
updated: 2026-10-22
synced_commit: 650b626
sources: [VEN/src/services/heuristics.rs, VEN/src/profile/heuristics.rs, VEN/src/tasks/heuristics_job/mod.rs, VEN/src/tasks/daily_gate.rs, VEN/src/state/heuristics.rs, VEN/src/entities/design_vocabulary.rs, VEN/src/services/forecast.rs, VEN/src/assets/base_load.rs, VEN/src/simulator/tick_inputs.rs, VEN/src/tasks/sim_tick/context.rs, VEN/src/tasks/history_sampler/mod.rs, VEN/src/controller/report_intervals.rs, VEN/src/controller/reporter.rs, docs/architecture/forecasting_model.md, docs/architecture/real_measurement_mqtt.md, docs/history/project_journal.md]
tags: [heuristics, forecasting, baseline, baseline-reports]
---

# Heuristics Pipeline (learned baselines)

The VEN learns a per-asset behavioural heuristic from its own persisted history ([[history-store]]) and
hands it to the asset that owns the forecast, which feeds it to the [[milp-planner]] as a per-slot baseline
instead of a flat scalar.

## The signal: base load (`assets/base_load.rs`)

`base_load` is the site's unmetered background consumption — standby draw, lighting, plugged-in
appliances, anything not modelled as its own asset. It is the *only* asset this pipeline learns for
(`HEURISTIC_ASSET_IDS`), and it is genuinely unsimulatable: its driver is occupant behaviour, which no
physics model predicts. Learning the statistical regularity from history is therefore the correct
permanent tool, not a placeholder (`docs/architecture/forecasting_model.md`). A second virtual asset,
`site-residual` (grid meter minus modelled assets), no longer exists: in a real deployment `base_load` is itself
derived as `grid_true − Σ(other measurements)`, which forces such a residual to zero by construction.

Live precedence (`BaseLoad::natural_base_kw`): a fresh measured MQTT reading wins, else the site's own
learned heuristic, else the synthetic profile plus appliance-noise model as a cold-start last resort. The same
function serves the tick and `SimState::peek_base_load_kw`, so the preview cannot diverge from the tick.

## The phenomenon: configured appliance noise

BaseLoad supports a `base_load.spikes` list in the profile (empty by default): each spike is a
**trapezoidal** daily pulse — flat plateau at `amplitude_kw`, linear ramps, day-to-day jitter in timing and
magnitude, optional weekday restriction, and a per-day firing `probability`. A trapezoid rather than a Gaussian
because its energy is directly `≈ amplitude_kw × (duration_h − ramp_h)`, settable to match a real appliance
session; Gaussian tails make the integral uncontrollable. This gives the learner a realistic, non-flat signal
to recover.

## The learner: `services/heuristics.rs` (application ring)

`learn_asset_heuristics(history, asset_id, now, cfg, previous)` is a pure aggregation over the asset's
`tick_samples` in the rolling window:

- **Seven weekday buckets**, each a 24-hour curve of recency-weighted mean power (`0` = Monday … `6` = Sunday).
  Each cell is **shrunk toward the overall mean**: the cell mean gets weight `effective_days / (effective_days +
  k)` (`shrinkage_k_days`), so a sparsely observed weekday-hour borrows from the whole profile instead of
  fitting noise, and a well-observed one stands on its own data. This replaced the earlier weekday/weekend
  two-bucket split, which existed only because seven raw buckets starved on a short seeding window.
- **EWMA recency** weighting (`ewma_halflife_days`) over a `rolling_window_days` window.
- A **seasonal factor**: the recency-weighted mean of the last 30 days over the overall mean.
- A **cold-start gate** (`min_samples_for_confidence` ticks) below which it returns `Ok(None)` and the flat
  fallback stays in place.
- `recent_mean_abs_error_kw` (R-60): how far the *previous* heuristic's predictions were from each tick's actual
  power, so a run can say whether the learned profile is converging. Instrumentation only; nothing acts on it yet.

The knobs are a flat top-level `heuristics:` profile section (`profile/heuristics.rs`, validated at boot), not
per-asset, because they tune the algorithm rather than an asset's physics. Defaults: 56-day window, 28-day
EWMA half-life, shrinkage `k` = 2.5 days, 100 samples ([[reliability-and-config]]).

The result is `AssetHeuristics` (`entities/design_vocabulary.rs`): `daytime_profile_kw: [Vec<f64>; 7]` ×
`seasonal_factor`, sampled by `sample_kw(slot_t)`, the one formula the planner, the forecast timeline and the
reports share, so they cannot diverge into several sampling rules.

## Scheduling

`tasks/heuristics_job/` is a supervised background task that wakes hourly and runs the aggregation once per
day (`DailyGate`, shared with the history sampler's day boundary; it fires on its first check too, so a fresh
start does not wait a day). It runs the learner on a blocking thread, passing the previously stored heuristic,
and stores each non-`None` result in `AppState` (`asset_heuristics`); failures are log-and-continue, never
blocking the control loop. It starts only when the history store is present. The synthetic backfill generator
(`generate_synthetic_backfill`) is test-only; there is no debug route that preloads history.

## The consumers

The heuristic reaches the asset through the tick: `AssetTickFeeds` carries it per base load
(`BaseLoad.heuristic`), and the asset itself answers "what will I draw at *t*" (`forecast_kw_at`: the heuristic
when present, else the static configured baseline), per the asset-competence rule that interpretation has one
authority ([[asset-layer]]).

- **Planner** ([[milp-planner]]): the base load builds its own MILP context from `forecast_kw_at`, so each plan
  slot samples the learned curve instead of repeating a flat scalar across the horizon.
- **Forecast timeline** (`services/forecast.rs::build_heuristic_forecasts`): the same sampling feeds the
  Controller tab's future-horizon lines in [[ven-ui]], which show real daily structure once history is seeded.
- **Forecast accuracy**: `record_forecast_accuracy_samples` records the near and far predicted values of every
  forecast asset in a plan (a load's prediction is its learned heuristic) for later reconciliation against actuals
  ([[history-store]]). That is the tool for checking whether the learned profile converges.
- **BASELINE reports** (`controller/report_intervals.rs::build_baseline_report_intervals`): a VTN report
  obligation requesting `payloadType: "BASELINE"` gets each interval's value from `AssetHeuristics::sample_kw`
  summed across assets, declared as energy for the interval, submitted alongside the concurrent `USAGE`
  report ([[openadr-interface]]). It reuses the same event-blind sampling the planner and timeline use rather
  than a fresh model: the heuristic never saw the event, so it needs no adjustment to serve as the
  counterfactual "what if this event weren't happening". Each interval also carries a `DATA_QUALITY` payload
  tagged `"HEURISTIC"` — provenance, not a computed confidence (a real confidence model is a deliberate
  non-goal). Downstream, `experiments/kpi.py`'s `event_impact_kwh` diffs archived BASELINE vs. USAGE reports per
  VEN to quantify one event's actual impact in kWh ([[experiment-harness]]).

## Composes with [[real-measurement-mqtt]]

This pipeline does not distinguish where a `tick_samples` row's `power_kw` came from: the history sampler
records whatever `entry.last_power_kw` was that tick, whether the synthetic `appliance_noise_kw` model or a
real, live-measured reading. Once the measurement feed's `base_load_enabled` gate substitutes a measured
value into the live tick, it flows into `tick_samples` and therefore into this learner with no extra plumbing.
`GET /forecast`'s `base_load` entry then reports `"source":"HEURISTIC"` with the learned profile driving the
planner's forecast. The convergence timeline follows the EWMA half-life and window above, and a feed dropout
silently re-mixes synthetic samples back in with no provenance tag to detect it.
