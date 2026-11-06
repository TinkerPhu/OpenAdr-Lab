# Forecast publication

The `lab-forecaster` container publishes the exogenous forecasts (PV generation, base load) as
retained MQTT messages, so any consumer (the VEN, the home project's advisor) reads one source
over MQTT only. The container needs the broker and its own cache; no InfluxDB.

## Topics

Under `<root>/forecast/<site>/`, all retained, QoS 1:

| Topic | Content |
|---|---|
| `pv/<array_id>/power_kw` | natural PV generation of one array (the id of the PV asset in the VEN profile) |
| `pv/total/power_kw` | the sum of the arrays, slot by slot |
| `base_load/power_kw` | base-load consumption, hourly steps, 48 h (published when the profile switches the base-load measurement on) |
| `status` | state, input ages, snow state per array, `producer_id`; last will sets `offline` |

`total` is reserved and is never an array id. Names are built only by
`ForecastTopics` (`lab-forecast/src/forecast_payload.rs`), which refuses wildcards and separators.

## Payload

One JSON object per series: `schema_version`, `generated_at`, `valid_until`, `model_version`,
`producer_id`, `start`, `step_s`, `values` (average kW per step), `confidence`, `coverage_h`, and the
declarations `quantity`, `unit`, `sign`, `reading` (plus `curtailment` for PV). The wire carries
the whole contract; see `docs/reference/WIRE_PROFILE.md#v4`. Payloads are built by
`ForecastPayload::pv_natural` / `base_load`, which cannot produce an undeclared one, and read by
`ForecastPayload::parse`, the one validator for producer and consumer.

## Freshness and health

The producer republishes at every recompute, also when nothing changed, so `generated_at` is a
heartbeat. A consumer treats a series past `valid_until` as stale (`is_fresh`). Status states:
`ok`, `cold_start`, `weather_stale`, `degraded`, `offline`. One producer per site: a second
container refuses to start while another producer's status is live (`ForecastStatus::is_live`).

## Where the logic lives

`lab-forecast` holds the PV physics (`solar`, `pv_snow`), the weather types and parse
(`weather`, `weather_wire`), the profile parameters (`pv_params`) and this payload. It reads no
clock, socket or file. The VEN re-exports these and keeps an in-process fallback.

## Running it

The service `lab-forecaster` (`forecaster/Dockerfile`, compose service in `VEN/docker-compose.yml`; `test-forecaster` in
`tests/docker-compose.test.yml`) reads its settings from the environment (`forecaster/src/config.rs`):

| Variable | Meaning | Default |
|---|---|---|
| `FORECASTER_MQTT_HOST`, `_PORT`, `_USERNAME`, `_PASSWORD` | the broker (plaintext) | host required, port 1883 |
| `FORECASTER_MQTT_ROOT` | `<root>` of every topic: weather and measurement in, forecast out | `openadr-lab` |
| `FORECASTER_SITE_ID` | `<site>` of every topic | required |
| `FORECASTER_PROFILE_PATH` | a VEN profile YAML; its `type: pv` assets with a `weather:` block are the arrays | required |
| `FORECASTER_PRODUCER_ID` | who publishes; a second container with another id is refused while this one is live | hostname |
| `FORECASTER_FRESHNESS_MINUTES` | `valid_until - generated_at`, and the oldest weather forecast still used | 90 |
| `FORECASTER_RECOMPUTE_SECONDS` | heartbeat besides every new weather forecast | 900 |
| `FORECASTER_MODEL_VERSION` | names the PV model in each payload | `pv-weather-1` |
| `FORECASTER_CACHE_PATH` | file keeping the base-load history across restarts; empty turns it off | `/data/base_load_cache.jsonl` |

It reads `<root>/weather/<site>/forecast` and, when a profile array switches `measurement` on,
`<root>/measurement/<site>/pv` and, when the base-load asset switches `measurement` on, `<root>/measurement/<site>/base_load`; it needs nothing else. A `SIGTERM` publishes the retained status `offline`.
Per hourly weather sample the value is the weather model's PV output at the start of that hour, declared as the average
over the step; the first sample is the weather feed's already-past hour, so a reader skips slots before now.

Where the code is: `forecaster/src/engine.rs` (the decision), `dispatch.rs` (message routing and the single-producer guard),
`config.rs`, `runtime.rs` (the broker session). `base_load.rs` keeps the measured load in 10-minute buckets and calls the shared learner (`lab-forecast/src/base_load_learner.rs`, the one the VEN uses); `cache.rs` is the cache file text. The VEN reading these topics follows.

## Base load

`base_load/power_kw` is hourly, 48 values, starting at the hour that contains `generated_at`; `values[i]` is the learned typical
consumption for that weekday and hour, times a seasonal factor. Until the history clears the learner's gate (100 samples) the
series is flat at the mean of what exists, or 0.4 kW with no history at all, `model_version` is `base-load-cold-start-1`, and the
status state is `cold_start`; a learned series says `base-load-profile-1`. The base load is published also while the weather is stale.
Deleting the cache file loses only learned history.

## The VEN reads them

A VEN with `FORECAST_MQTT_HOST` set (ven-1 on Node1; `FORECAST_MQTT_PORT`, `_ROOT`, `_SITE_ID` as for the weather feed)
subscribes to the topic of each PV array of its profile and to `base_load/power_kw` (`VEN/src/forecast_feed.rs`). Every message
goes through `ForecastPayload::parse`; the latest valid payload per series is kept in `AppState` (`ReceivedForecasts`,
`lab-forecast/src/received_forecasts.rs`), a refused message is recorded with its reason and leaves the older payload in place.

- **PV:** each tick, `resolve_weather_pv_for_tick` asks `choose_pv_series` (`lab-forecast/src/received_pv.rs`): the received
  series while `now <= valid_until` and declared `generation_positive`, else the VEN's own weather computation (the physics run
  only then). The chosen slots go to the PV asset as before (`TickOverrides.pv_weather_forecast`); the asset stays the only
  interpreter. A received series carries the forecaster's snow state, so its slots leave the asset's snow observation nothing
  to add. The `/forecast` API uses the same choice (`build_received_pv_forecast`).
- **Base load:** the series goes to the base-load asset whose measurement the profile switches on
  (`TickOverrides.base_load_received`); `BaseLoad::forecast_kw_at` answers from it inside its steps, from the learned heuristic
  beyond, and from the profile baseline last.
- **Fallback:** nothing configured, nothing received, a stale or refused series: the VEN computes its own, as before the
  forecaster existed. Units are kW both ways; the sign is the VEN's own (generation positive inside the asset, negated in
  `AssetForecast`) and conversion happens only where the payload becomes slots.
- **Shown:** `GET /weather` carries `forecast_sources` (source in use per series, `model_version`, age of the newest message,
  refusal reason); the VEN UI's Weather page lists them (`ForecastSourcePanel`).
- **Scored:** `forecast_accuracy_samples` has a `model_version` column (schema v13); a sample whose series came from the
  forecaster names its model, one from the VEN's own computation is NULL.
