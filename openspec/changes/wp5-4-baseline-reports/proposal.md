## Why

SG-3 (VTN-side report-usefulness evaluation) is currently "directional only": the VTN
recorder archives `USAGE`/`USAGE_FORECAST` reports but there is no counterfactual to compare
them against, so nobody can say how much a DR event actually changed a VEN's consumption.
The VEN already computes a heuristic forecast (WP5.2, `AssetHeuristics`) and already tracks
forecast-vs-actual accuracy internally (`forecast_accuracy_samples`) — but never submits a
`BASELINE` report to the VTN, so this data never reaches the M&V evaluation. This is the
last piece needed to turn SG-3 from directional into M&V-grade, and it's newly practical to
verify: the VTN recorder pipeline that would carry this data was found completely dead for 9
days (fixed 2026-08-10, see `docs/history/project_journal.md`) and is now confirmed healthy.

Investigation while scoping this change found `docs/plans/roadmap/phase-5-forecast-and-baseline.md`
(WP5.4 items 2–3) is stale: `reportDescriptor.historical` parsing, forecast-vs-measurement
routing, and `IMPORT_CAPACITY_RESERVATION`/`EXPORT_CAPACITY_RESERVATION` reporting from the
`SiteFlexibilityEnvelope` are all already implemented (`controller/reporter.rs`, R-15, WP3.6
§8.8). This proposal is scoped to the one genuinely missing piece — BASELINE reports — plus
quality metadata, which can build directly on the existing `forecast_accuracy_samples`
infrastructure rather than being invented fresh.

## What Changes

- VEN computes a `BASELINE` report payload during/after an active event window: the
  heuristic forecast (`AssetHeuristics`, same source as `USAGE_FORECAST`) evaluated *as if
  the event were not active* — the counterfactual net site power for that window.
- `BASELINE` is submitted alongside the existing `USAGE` measurement report for the same
  event/interval, not as a replacement.
- `experiments/kpi.py` gains `event_impact_kwh = Σ(baseline − actual)` per event, computed
  from the recorder's archived `BASELINE` + `USAGE` report pairs.
- Report payloads (both `BASELINE` and existing forecast payloads) carry a quality/confidence
  field, reusing the sample-count/variance signal `AssetHeuristics` already tracks internally
  — not a new confidence model, just surfacing the existing one on the wire.
- BDD: one new scenario exercising a `BASELINE` report during an active event, asserting the
  payload lands correctly on the recorder side.

## Capabilities

### New Capabilities
- `baseline-reports`: VEN-side computation and OpenADR submission of `BASELINE` counterfactual
  reports during active events, plus the quality-metadata field riding along on report
  payloads, plus the `kpi.py` consumption side that turns archived BASELINE/USAGE pairs into
  a per-event impact-in-kWh number.

### Modified Capabilities
(none — no existing `openspec/specs/` capabilities exist yet in this repo; the
already-implemented historical/forecast-routing/capacity-reservation behavior in
`controller/reporter.rs` is pre-existing and out of scope for this change)

## Impact

- **Backend**: `VEN/src/controller/reporter.rs` (new `build_baseline_report`-style function,
  reusing `build_measurement_report_for_obligation`'s obligation-driven path), `VEN/src/services/forecast.rs`
  or a sibling module (baseline-forecast computation reusing `AssetHeuristics`), `VEN/src/entities/history.rs`
  if a new report-obligation payload type constant is needed.
- **Experiment harness**: `experiments/kpi.py` (new `event_impact_kwh` computation, reading
  `recorder-reports_received.csv`'s `BASELINE` rows against matching `USAGE` rows).
- **Tests**: new unit tests for the baseline-forecast computation (test-first) and one new
  BDD scenario (`tests/features/`).
- **Docs**: `docs/plans/roadmap/phase-5-forecast-and-baseline.md` gets deleted once this
  change is implemented and tested (per the project's plan-lifecycle rule), its remaining
  substance folded into `docs/architecture/weather_forecast.md`/a new forecasting-baseline
  concept doc and `wiki/`.
- **No breaking changes**: `BASELINE` is a new, additive payload type; existing `USAGE`/
  `USAGE_FORECAST`/capacity-reservation reporting is untouched.
