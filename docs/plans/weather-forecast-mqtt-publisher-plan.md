# Weather Forecast MQTT Publisher — Implementation Plan for `data_acquisition`

**This file is meant to live in the `data_acquisition` repository**
(`C:\DriveD\Tinker\Docker_DataAcquisition` locally / `/srv/docker/data_acquisition`
on Pi4-Server — its own git project, not part of `OpenAdr-Lab`). It is
self-contained: everything needed to implement the publisher is in this one
document. **Re-plan note**: this supersedes an earlier version of this
document written against a stale snapshot of the production code (module
names, polling framework, and test suite have all since changed — see
"What changed since the last plan" below).

## Why this exists

`OpenAdr-Lab`'s VEN service has a complete, tested MQTT **consumer** for
weather forecasts (`WeatherForecastPort`, an MQTT adapter, a `GET /weather`
diagnostic route, and planner integration) — but nothing publishes to the
topics it subscribes to. `data_acquisition` already fetches SRF Meteo
hourly and writes it to InfluxDB; this plan adds a second, parallel output
from the same fetched data: an MQTT publish in the schema below, alongside
the existing Influx write, not instead of it.

## What changed since the last plan (re-analysis)

The production code moved on since this was first investigated. Concretely:

- **`DataPoller/` → `DataRequests/`.** The module is renamed; e.g.
  `DataRequests/SrfWeatherRequests.py`, `DataRequests/DataPollerMain.py`.
- **A new generic polling framework**: `DataRequests/DataRequestHost.py`'s
  `DataRequestHost` class replaces the old hand-rolled `TriggeredThreadEx`
  list. Sources register via
  `host.append_data_poller(interval_s, thread_name, poller_fn, transformer_fn)`
  — `poller_fn(logger) -> Any` fetches raw data, `transformer_fn(logger, data) -> List[Point]`
  turns it into Influx points, which get pushed onto the shared
  `points_queue`. This is the extension point this plan hooks into.
- **SRF Meteo now has an hourly quota gate.** SRF's API quota is a
  "handful of calls/day." `DataRequests/DataPollerMain.py` now checks
  every 60s (`is_srf_weather_poll_due`) but only actually calls the SRF
  API once per hour, inside a fixed window (minutes 10–15 past the hour —
  `SRF_WEATHER_POLL_WINDOW_START_MIN`/`_END_MIN`), and only once per hour
  even across restarts (`last_served_hour_key` tracking). **This plan must
  never add a second, independent SRF API call** — it has to piggyback on
  the exact same fetched body the existing Influx path already gated and
  paid for.
- **`InfluxSender` is now a class** (`tinkerPy/InfluxSender.py`,
  constructor `InfluxSender(logger, points_queue, interval_s, bucket, url,
  org, token)`, `.start_processing()`/`.cancel()`), not the old
  module-level function. Irrelevant to this plan directly, but any old
  reference to the function form is stale.
- **The transform function's signature changed**: `transform_srfWeather_to_data_points(data_labels, body, logger)`
  — it now takes a leading `data_labels` tuple (unused for SRF, kept for a
  uniform shape shared with other transformers like `ShellyToInfluxDb`).
  The field mapping itself is unchanged (same fields, same `age_h`
  slicing, same `-1h` timestamp shift).
- **A real pytest suite now exists** (`tests/`, `pyproject.toml`'s
  `[tool.pytest.ini_options]` → `testpaths = ["tests"]`), with established
  conventions this plan's own tests must follow — see "Testing" below.
- **A parallel `data_acquisition_test_service`** now exists in
  `docker-compose.yml` (image `data_acquisition_test`, env file
  `.env.test`) specifically for verifying refactored code live on Pi4
  before cutover, with safety toggles (`ENABLE_SRF_WEATHER_POLLING=false`,
  `DISABLE_INFLUX_WRITE=true` by default in `.env.test`/`demo.env.test`).
  This plan adds an analogous toggle for the new feature (see Config).

## What already exists to build on

- **`DataRequests/SrfWeatherRequests.py`** — OAuth token fetch (cached,
  refreshed every 6 days) + `request_srf_weather(logger, username,
  password)`, returns the raw SRF JSON body for `47.4491,7.8081`. Nothing
  to change here; the publisher consumes the same body this already
  fetches.
- **`DataRequests/DataPollerMain.py`** — registers the SRF poller:
  ```python
  drh.append_data_poller(60, "SrfWeather_polling",
                          srf_weather_poll_if_due,
                          srf_weather_transform)
  ```
  `srf_weather_poll_if_due(lg)` returns `None` when outside the poll
  window/already served this hour (quota gate), or the raw SRF body when
  it actually fetches. `srf_weather_transform(lg, da)` returns `[]` for
  `da is None`, else calls `SrfWeatherToInfluxDb.transform_srfWeather_to_data_points(None, da, lg)`.
  **This is the hook point**: extend `srf_weather_transform` to also
  publish MQTT from the same `da`, as a side effect, before/after building
  the Influx points — never gated separately, never triggering a second
  SRF fetch.
- **`InfluxTransformers/SrfWeatherToInfluxDb.py`** — the field-mapping
  reference to mirror for the new MQTT transform (same `hourIndex =
  datetime.now().hour` slicing, same `[hourIndex:hourIndex+49]` window,
  same `age_h = i`, same `-1h` timestamp shift because SRF timestamps mark
  the *end* of the forecasted hour).
- **`InfluxTransformers/examples/srfMeteo.json`** — the existing test
  fixture; reuse it for the new transform's tests too (same shape, same
  frozen-hour convention already established in
  `tests/InfluxTransformers/test_srf_weather_to_influx_db.py`).
- **No existing `paho-mqtt` usage in this repo** (confirmed by search) —
  needs adding fresh. The pattern to follow lives in the *separate*
  `mqtt_bridge` project on Pi4-Server (`RestToMqttBridge/RestToMqtt.py`):
  `paho.mqtt.client.Client(mqtt.CallbackAPIVersion.VERSION2, client_id=...)`,
  `client.connect(broker, port, keepalive)`, `client.publish(topic, json_str, qos=1, retain=True)`.
  That file isn't in this repo, so re-implement the same small pattern
  here rather than importing across projects.
- **Broker**: `mosquitto` container, already on the same `influxdb_network`
  docker network as both `data_acquisition_service` and
  `data_acquisition_test_service` (confirmed in `docker-compose.yml`) — no
  network changes needed, reachable at hostname `mosquitto`, port 1883.

## Wire contract — copied in full from `OpenAdr-Lab`'s `docs/plans/weather-forecast-plugin.md`

Unchanged from the original design; this is the exact contract the VEN-side
MQTT adapter already implements and tests against. Follow it byte for
byte — the VEN adapter validates every field and rejects malformed
messages (logs and drops, never crashes), so a schema mismatch here means
silently no data reaching VEN, not a visible error on either side.

### Transport conventions (apply to every topic below)

- **Encoding**: UTF-8 JSON, no BOM.
- **Timestamps**: RFC 3339 / ISO 8601, **UTC only**, always with an explicit
  `Z` suffix — never a numeric offset, never local time. SRF's raw
  `local_date_time` field carries a `+02:00`-style offset
  (Europe/Zurich) — **convert to UTC before publishing.** This is the
  single most important correctness rule in this whole plan: an
  unconverted offset is a silent off-by-one-hour bug during DST
  transitions, not a parse error the VEN side would ever catch.
- **Topic naming**: `<root>/weather/<site_id>/<subtopic>`. Suggested
  values: `<root>` = `openadr-lab`, `<site_id>` = a slug for this house
  (e.g. `main-roof` or `zunzgen`) — must match whatever VEN's
  `WEATHER_MQTT_ROOT`/`WEATHER_MQTT_SITE_ID` env vars are configured to on
  the consuming side.
- **QoS**: 1 (at-least-once) on every topic.
- **Retained**: `true` on every topic — a VEN instance that (re)connects
  gets the last known state immediately.
- **Forward compatibility**: never remove/repurpose a field without
  bumping `schema_version`'s major component; unknown extra fields are
  fine to include (the consumer ignores them).
- **On fetch failure**: do **not** touch the retained `forecast` topic.
  Only publish `forecast` on a successful fetch. Signal failure via the
  `status` topic instead (below).

### Topic 1 — `<root>/weather/<site_id>/forecast`

**Cadence**: once per successful fetch — i.e. once per hour, exactly when
`srf_weather_poll_if_due` actually returns a body (never more often; there
is no reason to publish faster than the source data changes, and no
reason to add a timer beyond the existing hourly gate).

**Schema** (JSON Schema, draft 2020-12):

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://openadr-lab.example/schemas/weather-forecast-message.json",
  "title": "WeatherForecastMessage",
  "type": "object",
  "required": ["schema_version", "source_id", "location", "fetched_at", "samples"],
  "properties": {
    "schema_version": {
      "type": "string",
      "pattern": "^[0-9]+\\.[0-9]+\\.[0-9]+$",
      "examples": ["1.0.0"]
    },
    "source_id": {
      "type": "string",
      "description": "e.g. \"srf_meteo\".",
      "minLength": 1
    },
    "location": {
      "type": "object",
      "required": ["latitude_deg", "longitude_deg"],
      "properties": {
        "latitude_deg":  { "type": "number", "minimum": -90,  "maximum": 90 },
        "longitude_deg": { "type": "number", "minimum": -180, "maximum": 180 }
      },
      "additionalProperties": false
    },
    "fetched_at": {
      "type": "string",
      "format": "date-time",
      "description": "UTC timestamp of the upstream API call this message reports."
    },
    "samples": {
      "type": "array",
      "minItems": 1,
      "description": "Ascending by valid_at.",
      "items": {
        "type": "object",
        "required": ["valid_at", "age_h", "temperature_c", "ghi_w_m2"],
        "properties": {
          "valid_at": { "type": "string", "format": "date-time" },
          "age_h": {
            "type": "integer", "minimum": 0, "maximum": 240,
            "description": "0 = most recent actual/\"fact\"; 1+ = hours ahead."
          },
          "temperature_c":  { "type": "number", "minimum": -60, "maximum": 60 },
          "ghi_w_m2":       { "type": "number", "minimum": 0,   "maximum": 1500,
            "description": "Global Horizontal Irradiance, unprojected. VEN performs the panel-plane transposition; this plugin must not." },
          "wind_speed_kmh": { "type": "number", "minimum": 0,   "maximum": 300 },
          "rain_prob_pct":  { "type": "number", "minimum": 0,   "maximum": 100 },
          "new_snowfall_cm": { "type": "number", "minimum": 0,  "maximum": 200 },
          "sky_condition": {
            "type": "string",
            "enum": ["clear", "mostly_clear", "partly_cloudy", "overcast",
                     "fog", "rain", "sleet", "snow", "thunderstorm", "unknown"]
          },
          "irradiance_variability": {
            "type": "number", "minimum": 0, "maximum": 1,
            "description": "0 = uniform sky for the whole hour, 1 = maximally broken/alternating sky."
          }
        },
        "additionalProperties": true
      }
    }
  },
  "additionalProperties": true
}
```

Example message (using this site's actual coordinates, already used
elsewhere in this repo's `.env`/candidates):

```json
{
  "schema_version": "1.0.0",
  "source_id": "srf_meteo",
  "location": { "latitude_deg": 47.4491, "longitude_deg": 7.8081 },
  "fetched_at": "2026-07-19T05:54:48Z",
  "samples": [
    {
      "valid_at": "2026-07-19T05:00:00Z",
      "age_h": 0,
      "temperature_c": 15.0,
      "ghi_w_m2": 0.0,
      "wind_speed_kmh": 4.0,
      "rain_prob_pct": 10.0,
      "sky_condition": "clear",
      "irradiance_variability": 0.0
    },
    {
      "valid_at": "2026-07-19T06:00:00Z",
      "age_h": 1,
      "temperature_c": 16.0,
      "ghi_w_m2": 97.0,
      "wind_speed_kmh": 4.0,
      "rain_prob_pct": 14.0,
      "sky_condition": "partly_cloudy",
      "irradiance_variability": 0.6
    }
  ]
}
```

### Topic 2 — `<root>/weather/<site_id>/status`

Exists so VEN can distinguish "no new forecast due yet" (normal — SRF is
only fetched once/hour) from "the publisher process died."

**Cadence**: on every status change **and** unconditionally at least every
5 minutes — independent of the hourly SRF fetch gate; this needs its own
timer. Register this topic as the MQTT client's **Last Will and
Testament** at connect time (payload matching `status: "offline"` below)
— `paho-mqtt`'s `client.will_set(topic, payload, qos=1, retain=True)`
called *before* `client.connect(...)`.

**Schema**:

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://openadr-lab.example/schemas/weather-status-message.json",
  "title": "WeatherStatusMessage",
  "type": "object",
  "required": ["schema_version", "source_id", "ts", "status"],
  "properties": {
    "schema_version": { "type": "string", "pattern": "^[0-9]+\\.[0-9]+\\.[0-9]+$" },
    "source_id": { "type": "string", "minLength": 1 },
    "ts": { "type": "string", "format": "date-time" },
    "status": {
      "type": "string",
      "enum": ["ok", "stale", "error", "offline"],
      "description": "ok = last fetch succeeded within the expected cadence window. stale = alive but upstream hasn't returned fresh data. error = last fetch attempt failed. offline = LWT payload."
    },
    "last_successful_fetch_at": { "type": "string", "format": "date-time" },
    "consecutive_failures": { "type": "integer", "minimum": 0 },
    "message": { "type": "string", "description": "Optional free-text detail, e.g. \"upstream HTTP 503\". Never parsed programmatically." }
  },
  "additionalProperties": true
}
```

Example:

```json
{ "schema_version": "1.0.0", "source_id": "srf_meteo", "ts": "2026-07-19T06:05:00Z",
  "status": "ok", "last_successful_fetch_at": "2026-07-19T05:54:48Z", "consecutive_failures": 0 }
```

### Summary table

| Topic | Cadence | Retained | QoS | LWT |
|---|---|---|---|---|
| `.../forecast` | once per successful SRF fetch (hourly, gated by the existing quota window); skip entirely on failure | yes | 1 | no |
| `.../status` | on every status change, and unconditionally ≥ every 5 min, on its own timer | yes | 1 | yes, `status:"offline"` |

## Field mapping — SRF hourly sample → wire schema sample

`body['forecast']['60minutes'][hourIndex:hourIndex+49]` (already exactly
what `SrfWeatherToInfluxDb.transform_srfWeather_to_data_points` slices)
maps directly:

| SRF field | Wire field | Notes |
|---|---|---|
| `local_date_time` (shift `-1h`, per the existing transform) | `valid_at` | **Convert to UTC.** Reuse the exact same `parse_datetime(h['local_date_time']) + timedelta(hours=-1)` shift already in `SrfWeatherToInfluxDb.py`, then convert to UTC before formatting with a trailing `Z`. |
| array index `i` (already `age = i` in the existing transform) | `age_h` | Reuse directly — `i=0` is the fact hour, `i=1..48` forward. |
| `TTT_C` | `temperature_c` | Already used for Influx. |
| `IRRADIANCE_WM2` | `ghi_w_m2` | Already used — Global Horizontal Irradiance, unprojected. Do not transpose it here. |
| `FF_KMH` | `wind_speed_kmh` | Not currently sent to Influx by this transform, but already in the raw API response. |
| `PROBPCP_PERCENT` | `rain_prob_pct` | Ditto. |
| `FRESHSNOW_CM` | `new_snowfall_cm` | Already captured as `snow_cmh` in the Influx schema — same source field. |
| `SYMBOL_CODE` | `sky_condition` | **Needs translation** — table below. |
| `SUN_MIN` | `irradiance_variability` | **Needs a formula** — below. |

### `sky_condition` mapping (from SRF's own commercial API documentation)

SRF's icon code: **sign = day/night rendering hint, magnitude = the actual
condition.** Map on `abs(code)` and discard the sign — VEN derives
day/night from solar position itself.

```python
def srf_symbol_to_sky_condition(code: int) -> str:
    """Translate an SRF SYMBOL_CODE into the generic wire-contract vocabulary.
    Sign is a day/night rendering hint (discarded); magnitude is the condition.
    Legend from SRF's commercial API PDF (2023-05-10 srf_meteo_api_commercial_eng_dok.pdf).
    """
    mapping = {
        1: "clear",            # sonnig
        10: "mostly_clear",    # ziemlich sonnig
        3: "partly_cloudy",    # teils sonnig — SRF's own word for "changing"
        19: "overcast",        # bedeckt
        20: "rain",            # regnerisch / bewölkt: etwas Regen
        4: "rain",             # Regenschauer
        25: "rain",            # Regenschauer
        5: "thunderstorm",     # Regenschauer mit Gewitter
        21: "snow",            # Schneefall
        6: "snow",             # Schneeschauer
        22: "sleet",           # Schneeregen
        8: "sleet",            # Schneeregenschauer
        2: "fog",              # Nebelbänke
        17: "fog",             # Nebel
    }
    return mapping.get(abs(code), "unknown")  # never guess an unmapped code
```

### `irradiance_variability` formula

`SUN_MIN` (minutes of actual sunshine within the 60-minute hour, 0–60) is
a direct, continuous, intra-hour clear/cloudy fraction. Peaked-at-the-middle
shape: 0 or 60 (uniform sky, either fully clear or fully overcast) → 0.0;
30 (sky genuinely alternating within the hour) → 1.0.

```python
def irradiance_variability(sun_min: float) -> float:
    """0.0 = uniform sky all hour (clear or overcast), 1.0 = maximally broken."""
    return 1.0 - abs(2.0 * (sun_min / 60.0) - 1.0)
```

## Implementation steps

1. **Add `paho-mqtt` dependency.** Add to `requirements.txt` (pin
   `paho-mqtt==2.1.0`, matching the version the sibling `mqtt_bridge`
   project already uses, for consistency across this stack's containers).
   If `pyproject.toml`'s `[project].dependencies` is the actual
   source-of-truth for this repo's poetry environment (it currently lists
   none, while `requirements.txt` carries pinned versions — check which
   one `Dockerfile`'s install step actually reads before assuming), add it
   there too / instead.
2. **Add config** to `.env` (production) and `demo.env.test`/`.env.test`
   (parallel test service):
   ```
   MQTT_BROKER=mosquitto
   WEATHER_MQTT_ROOT=openadr-lab
   WEATHER_MQTT_SITE_ID=<choose a slug for this site>
   ENABLE_WEATHER_MQTT_PUBLISH=true
   ```
   In `demo.env.test`/`.env.test`, set `ENABLE_WEATHER_MQTT_PUBLISH=false`
   by default (same safety posture as `ENABLE_SRF_WEATHER_POLLING=false`/
   `DISABLE_INFLUX_WRITE=true` already there) — flip it on deliberately,
   with a distinct `WEATHER_MQTT_SITE_ID` (e.g. `<site>-test`), only when
   actually verifying this feature, so test-container publishes never
   collide with the production retained topic.
3. **Add a small MQTT client module**, e.g.
   `DataRequests/WeatherMqttPublisher.py`:
   - One `paho.mqtt.client.Client` instance, created once (mirrors
     `InfluxSender`'s single-instance-at-startup shape), with
     `will_set("<root>/weather/<site>/status", json.dumps({...offline...}), qos=1, retain=True)`
     called *before* `client.connect(broker, 1883, keepalive=60)`, then
     `client.loop_start()` (paho's background network thread — do this
     once, not per-publish).
   - `publish_forecast(client, root, site_id, message: dict) -> None`:
     `client.publish(f"{root}/weather/{site_id}/forecast", json.dumps(message), qos=1, retain=True)`,
     wrapped in its own `try/except` (a publish failure must never affect
     the Influx write path).
   - `publish_status(client, root, site_id, status: str, **kwargs) -> None`: same shape.
   - `shutdown(client) -> None`: `client.loop_stop(); client.disconnect()`.
4. **Add the transform**, e.g.
   `InfluxTransformers/SrfWeatherToMqtt.py`,
   `transform_srfWeather_to_mqtt_message(body: dict) -> dict`, mirroring
   `SrfWeatherToInfluxDb.transform_srfWeather_to_data_points`'s exact
   `hourIndex`/slicing/timestamp-shift logic (same `datetime.now().hour`
   convention — do **not** introduce a different testability style here;
   mirror the existing monkeypatch-`datetime`-in-the-module pattern used
   by that file's own tests instead of adding an injectable-clock
   parameter this codebase doesn't otherwise use). Pure function (dict in,
   dict out) — no I/O, no MQTT import — so it's unit-testable without a
   broker or network, same as the Influx transform.
5. **Wire into `DataRequests/DataPollerMain.py`**: extend
   `srf_weather_transform(lg, da)` — after (or before) building `points`
   via the existing Influx transform, also:
   ```python
   if os.environ.get("ENABLE_WEATHER_MQTT_PUBLISH", "true").lower() != "false" and da is not None:
       try:
           message = SrfWeatherToMqtt.transform_srfWeather_to_mqtt_message(da)
           WeatherMqttPublisher.publish_forecast(mqtt_client, mqtt_root, mqtt_site_id, message)
       except Exception:
           lg.log("weather MQTT publish failed", LogLevel.ERROR, add_stacktrace=True)
   ```
   Construct `mqtt_client` once inside `DataPollerMain.main()` itself
   (this function already reads all its own config directly from
   `os.environ` rather than receiving it as parameters — e.g.
   `SHELLY_GRID_HOST`, `FRONIUS_HOST` — so constructing the MQTT client
   here, the same way, is the minimal-diff choice; it avoids touching
   `DataAcquisitionMain.py`'s signature and the existing
   `test_data_acquisition_main_wiring.py` fakes). Skip client construction
   entirely (log once, matching the `ENABLE_SRF_WEATHER_POLLING` disabled
   log line) when `ENABLE_WEATHER_MQTT_PUBLISH=false` or `MQTT_BROKER` is
   unset.
6. **Add the heartbeat as its own `DataRequestHost` poller** — the
   framework already supports an independent cadence per registered
   source via `interval_s`, so this needs no new mechanism:
   ```python
   if enable_weather_mqtt_publish:
       drh.append_data_poller(250, "weather_mqtt_heartbeat",
                               lambda lg: None,  # nothing to fetch
                               lambda lg, da: (WeatherMqttPublisher.publish_status(
                                   mqtt_client, mqtt_root, mqtt_site_id, "ok",
                                   last_successful_fetch_at=..., consecutive_failures=...
                               ), [])[1])  # always returns [] — nothing goes to Influx
   ```
   Track `last_successful_fetch_at`/`consecutive_failures` as simple
   module-level (or closure-captured, mirroring `srf_weather_last_served_hour`'s
   existing mutable-holder pattern) state updated by the fetch path in
   step 5, so the heartbeat reflects real fetch outcomes, not just "the
   process is alive."
7. **Update `main()`'s `quit()` closure** to call
   `WeatherMqttPublisher.shutdown(mqtt_client)` alongside the existing
   `drh.cancel(...)`.

## Testing

Follow this repo's established conventions exactly — see
`tests/conftest.py` (`logger` fixture → `RecordingLogger`),
`tests/InfluxTransformers/test_srf_weather_to_influx_db.py` (frozen-`datetime`
monkeypatch pattern, `srfMeteo.json` fixture), and
`tests/DataRequests/test_data_poller_main_wiring.py`/
`tests/integration/test_srf_weather_poller_integration.py` (fake-host /
end-to-end wiring style).

- **`tests/InfluxTransformers/test_srf_weather_to_mqtt.py`** (new, mirrors
  `test_srf_weather_to_influx_db.py` structure): reuse the same
  `srf_body`/`frozen_hour_5` fixtures; assert the produced message has 49
  samples, `age_h` 0..48, `valid_at` timestamps ending in `Z`, and that
  `ghi_w_m2`/`temperature_c`/etc. match the fixture's raw fields.
- **`tests/DataRequests/test_srf_symbol_mapping.py`** (new, pure-function
  unit tests): one assertion per row of the `sky_condition` mapping table,
  including that an unmapped code returns `"unknown"`; `irradiance_variability(0) == 0.0`,
  `(60) == 0.0`, `(30) == 1.0`.
- **Extend `tests/integration/test_srf_weather_poller_integration.py`**-style
  test: register the SRF poller exactly as `DataPollerMain.py` does, feed
  the fixture body through a **fake MQTT client** (a small recorder class,
  same shape as this test file's existing `FakeDataRequestHost`/
  `FakeInfluxSender` doubles elsewhere in the suite — record
  `.publish()` calls, assert on topic/payload, never open a real socket),
  and assert exactly one `forecast`-topic publish happens per fetch, with
  no second SRF API call.
- **Extend `tests/DataRequests/test_data_poller_main_wiring.py`**-style
  test (using its existing `FakeDataRequestHost`): assert
  `"weather_mqtt_heartbeat"` is registered by default and skipped when
  `ENABLE_WEATHER_MQTT_PUBLISH=false`, mirroring the existing
  `test_srf_weather_skipped_when_disabled_but_other_devices_still_registered`
  test exactly.
- **Manual verification** once deployed (Pi4, real broker):
  `mosquitto_sub -h Pi4-Server -t 'openadr-lab/weather/#' -v` should show
  the `status` heartbeat immediately (every ≤5 min) and the `forecast`
  topic after the next hourly SRF fetch window.
- **Parallel-test-service verification**: temporarily set
  `ENABLE_WEATHER_MQTT_PUBLISH=true` (and, if actually exercising the real
  SRF fetch too, `ENABLE_SRF_WEATHER_POLLING=true` — accepting that this
  spends one real quota unit) in `.env.test` with a distinct
  `WEATHER_MQTT_SITE_ID`, run `data_acquisition_test_service` alongside
  production, and check the distinct test topic — never touching the
  production retained topic while verifying.
- **End-to-end verification**: once a VEN instance has
  `WEATHER_MQTT_HOST`/`WEATHER_MQTT_ROOT`/`WEATHER_MQTT_SITE_ID` configured
  to match, `GET /weather` on that VEN (see `OpenAdr-Lab`'s
  `VEN/src/routes/weather.rs`) should show `status: "ok"` with the real
  forecast in `raw`.

## Non-goals (explicitly out of scope for this plan)

- Broker security (TLS, auth) — the existing Mosquitto deployment allows
  anonymous connections on its plaintext 1883 listener; acceptable for a
  trusted-LAN lab, revisit before any exposure beyond it (a password file
  already exists at `/srv/docker/mosquitto/config/pwfile`, unused).
- Any supplier other than SRF Meteo.
- Any change to the existing InfluxDB write path — this is purely additive.
- Any change to the SRF quota-protection gate itself
  (`is_srf_weather_poll_due`) — this plan strictly reuses it, never
  duplicates or bypasses it.
