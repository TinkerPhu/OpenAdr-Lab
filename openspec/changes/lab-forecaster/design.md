## Context

See proposal.md - Why. Decisions taken with the user (2026-11-03, user, as recommended): D1 shared code,
D2 Rust reading the VEN profile YAML, D3 VEN keeps its in-process fallback, D7 the forecaster owns the PV
snow-state observation, D8 a separate crate `lab-forecast`, D9 per-array topics plus a site total.

## Goals / Non-Goals

**Goals**: 48 h PV and base-load forecasts retained on MQTT; runs with the container alone, without InfluxDB;
the VEN and other consumers use the same payload. **Non-Goals**: EV, battery and heater forecasts (planner
decisions); an Influx backfill adapter (optional, off by default, a follow-up card if wanted); changing the
home project's digest or kiosk (reported as a card to the coordination session: "the digest serves them" is
theirs).

## Inventory of existing logic reused or consolidated

| Concept | Where | This change |
|---|---|---|
| PV forecast from weather (transposition, cell temperature, snow, inverter) | `VEN/src/entities/solar.rs` (`weather_pv_forecast_series`, `forecast_ac_kw`), `pv_snow.rs` | Moved to `lab-forecast`; the forecaster and the VEN fallback call the one function |
| Snow state observation | `PvSnowState::observed` (R-55), `VEN/src/state/pv_snow.rs`, `assets/pv_snow_cover.rs` | The observation function moves to `lab-forecast` and the forecaster runs it on the PV measurement (D7); the VEN keeps it only for the fallback path |
| Per-slot confidence | `services/forecast.rs::slot_confidence` | Moved with it; published in the payload |
| Base-load learner | `services/heuristics.rs::learn_asset_heuristics`, `AssetHeuristics::sample_kw` | Moved to `lab-forecast`, reading history through a narrow `TickSource` trait (`query_ticks` only) that the VEN's `HistoryPort` store and the forecaster's cache both implement |
| Weather payload parse | `VEN/src/weather.rs`, `entities/weather.rs` | One parser in `lab-forecast` |
| Incoming numbers | `lab_core::wire_contract::PayloadReader` reads OpenADR event payloads only | Not applicable to forecast topics; `ForecastPayload::parse` is their one validator |
| Forecast accuracy | `history_store/forecast_accuracy.rs` (`forecast_accuracy_samples`) | Reused by the VEN against the received series |
| Measurement topics and translation | `VEN/src/measurement.rs`, `measurement_translation.rs` | The forecaster reuses the translation; no second one |

Copies that cannot be consolidated in reach go to `TECHNICAL_DEBTS.md` in the same piece of work.

## Decisions

**D1 Shared code, no copy.** The PV physics and the learner move out of the VEN so the VEN and the forecaster
call the one implementation. Rejected: the forecaster depending on the VEN crate (couples it to VEN internals,
breaks the ring rules); a re-implementation (second copy).

**D2 Rust, site settings from the VEN profile YAML.** Array geometry and rating stay declared once in
`VEN/profiles/*.yaml`. The profile types the forecaster needs are shared. Rejected: own config (drift), Python
(cannot reuse the physics).

**D3 The VEN keeps the in-process fallback.** It uses the received forecast while fresh and its own computation
otherwise; same shared function, no logic copy.

**D7 The forecaster owns the snow-state observation.** The PV forecast depends on whether the panel is covered
with snow, which the PV asset observes from its measurement (R-55). The forecaster has the PV measurement
feed, so it runs the same observation and keeps its snow state in its cache; the published series therefore
equals what the VEN's own computation gives with the same measurement. The VEN's received series enters its PV
asset as an injected parameter, so the asset stays the only interpreter of its own forecast. Rejected: the asset
re-applying snow on a series the forecaster published without it (the forecaster would add nothing and the
physics would be split across two places).

**D8 A separate crate `lab-forecast`.** `lab-core` stays thin (two services depend on it for rules they must
agree on). `lab-forecast` holds the PV physics, snow observation, learner, forecast payload and its validator;
the VEN, the forecaster (and later others) depend on it. The VEN-specific types the code touches today
(`BaseLoad`, `AssetHeuristics`, `HistoryPort`, `TickSample`, `ts_rs` derives) are separated first: the learner
takes a plain slice of `LoadSample`s (not a trait: the VEN maps its ticks, the forecaster its buckets) and plain parameter structs, and the VEN keeps thin adapters. This is not a pure move; it
is the first, largest task and sized separately.

**D9 Topics, per array plus a site total.** Under `<root>/forecast/<site>/`:
`pv/<array_id>/power_kw` for each PV array declared in the profile, `pv/total/power_kw` (their sum),
`base_load/power_kw`, and `status`. All retained, QoS 1; `status` has a last will. A VEN reads its own array's
topic. `<array_id>` is the PV asset id from the profile.

**D4 Payload.** One JSON object per forecast topic:
`schema_version`, `generated_at`, `valid_until`, `model_version`, `producer_id`, `start`, `step_s`, `values`,
`confidence`, `quantity` (`power`), `unit` (`kW`), `sign` (`generation_positive` for PV,
`consumption_positive` for base load), `reading` (`average_over_step`), `coverage_h`, and for PV
`curtailment` (`natural`: the uncurtailed generation, the exogenous half). The wire carries the full contract;
the VEN converts to its internal sign convention only at its boundary (`entities/units.rs`). Mapping to the
home catalogue's signal names is the home project's job.

**D5 Cadence and freshness.** Recompute when a new weather forecast arrives and at least every 15 minutes
(base load). Republish every recompute, also when the series is unchanged, so `generated_at` is a heartbeat;
consumers judge freshness against `valid_until` (default `generated_at` + 90 minutes, settable). Injectable
clock throughout. The VEN's freshness limit for using a received forecast is `valid_until` itself.

**D6 Cache and cold start.** A cache file of 10-minute base-load buckets (JSON lines; SQLite was dropped as a heavy dependency for a few MB of rebuildable data), rebuilt from the live measurement topics; deleting it loses
nothing but learned history. A cold start publishes the cold-start profile with `model_version` saying so and
status `cold_start`, never a silent value.

**D10 Inputs.** Weather: `<root>/weather/<site>/forecast` and `/status` (existing contract). Measurement: the
PV and base-load topics the profile configures per VEN today (one PV topic and one base-load topic); the
forecaster takes them from the same profile keys. Where the profile configures no measurement, there is no
snow observation (state default) and the base load is the cold-start profile.

**D11 One producer per site.** The status topic carries `producer_id`. The container refuses to start while a
live status from another producer for the same site is retained and fresh; it logs it and exits non-zero.

**D12 Coverage.** `coverage_h` states the horizon actually delivered: at most what the weather feed provides
(PV) and 48 h (base load). It is never padded.

**D13 Invalid and unknown.** The VEN rejects a forecast with an unknown major `schema_version`, a missing
declaration (unit, sign, quantity) or a non-numeric value (by the shared validator `ForecastPayload::parse`; `PayloadReader` reads OpenADR event payloads and does not apply), keeps its fallback, and
records the rejection reason where its UI shows the source.

**D14 First slice (R-81, user, 2026-11-04; build approach: user).** Before the full forecaster, the container
publishes the PV forecast only: the series the VEN's REST plan serves today, per array plus the site total, with
the final payload contract and the status topic, retained. It is built on the shared code (the PV part of
`lab-forecast`: weather parse, `solar.rs`, `pv_snow.rs`, the payload constructor and validator), so the slice is
already the final producer and nothing is retired later. The base-load learner, the VEN consumption with
fallback, the UI surface and the accuracy scoring follow in the later slices. The advisor reads the topics over
MQTT only (their best-time step), and the digest's read of the VEN REST plan is then removed on their side.
Rejected: a publisher inside the VEN (a temporary second producer that would have to be retired, and it would
publish the VEN's forecast rather than the container's).

## Risks / Trade-offs

- Separating the code from VEN types touches many call sites: step 1 of the tasks keeps every existing test
  unchanged and green before any new behaviour is added.
- Measurement-anchored snow state lives now in two places (the forecaster, and the VEN for its fallback). The
  state itself is derived (not shared); the function is the one. The duplication of *state* is accepted; a
  later change can retire the VEN side if the fallback is dropped.

## Open Questions

- Influx backfill adapter: out of this change; file a card if wanted.
