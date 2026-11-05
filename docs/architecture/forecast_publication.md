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
