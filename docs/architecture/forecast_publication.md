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
| `base_load/power_kw` | base-load consumption (a later slice) |
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
| `FORECASTER_MODEL_VERSION` | names the model in each payload | `pv-weather-1` |

It reads `<root>/weather/<site>/forecast` and, when a profile array switches `measurement` on,
`<root>/measurement/<site>/pv`; it needs nothing else. A `SIGTERM` publishes the retained status `offline`.
Per hourly weather sample the value is the weather model's PV output at the start of that hour, declared as the average
over the step; the first sample is the weather feed's already-past hour, so a reader skips slots before now.

Where the code is: `forecaster/src/engine.rs` (the decision), `dispatch.rs` (message routing and the single-producer guard),
`config.rs`, `runtime.rs` (the broker session). Slice 1 publishes PV only; base load and the VEN reading these topics follow.
