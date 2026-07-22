# Weather Forecast Feature — Architecture

VEN ingests an external weather forecast over MQTT and uses it to compute a
physics-based PV generation forecast, as an alternative to the sin-model
forecast used when no weather feed is configured. This document is the
architecture reference for that feature: the transport, the Rust-side data
structures, the PV transposition physics, the sky-condition/variability
signal, the snow-cover model, and the MQTT wire contract a plugin publisher
must implement.

Implemented in `VEN/src/entities/weather.rs`, `entities/solar.rs`,
`entities/pv_snow.rs`, `entities/asset_params.rs` (`PvArrayGeometry`,
`PvForecastParams`), `controller/weather_port.rs`, `VEN/src/weather.rs` (the
MQTT adapter), `routes/weather.rs` (`GET /weather`), the VEN UI Weather tab
(`VEN/ui/src/pages/Weather.tsx`), and profile config (`profile/weather_pv.rs`,
the `weather_pv` YAML section in `VEN/profiles/ven-{1,2,3}.yaml`). The
production feed for ven-1/2/3 is the Zunzgen site, published by the
`data_acquisition` project's `WeatherMqttPublisher`/`SrfWeatherToMqtt`
modules from SRF Meteo data — see that project's own docs for the publisher
side; this document covers the VEN (consumer) side and the wire contract
both sides implement.

Known gaps and deferred accuracy improvements are tracked in
`docs/reference/TECHNICAL_DEBTS.md` (R-52 through R-56), not repeated here.

## Architecture: MQTT pub/sub

VEN subscribes to a well-known MQTT topic (`<root>/weather/<site_id>/forecast`)
publishing a documented JSON schema. Any process that publishes to that topic
in the agreed schema counts as a "plugin" — it doesn't have to be Rust, doesn't
have to be started or supervised by VEN, and doesn't have to be a single
well-defined artifact (a Node-RED flow, a Python cron job, or a local weather
station's own MQTT firmware all qualify equally).

This decouples VEN from any single supplier's transport, auth model, or
release cadence:

- **Maximum decoupling** — VEN never spawns, restarts, or version-matches a
  plugin process. It only needs a topic name and a schema.
- **Naturally push-based**, fitting weather data well — irradiance and
  temperature changes are event-like on the timescale VEN cares about
  (hourly), not something worth a tight polling loop.
- **Retained messages** — the broker (Mosquitto) keeps the last message on a
  topic, so a VEN instance that starts up or reconnects gets the last known
  forecast immediately.
- **Multiple plugins coexist** on different topics/sites without any VEN-side
  code change.
- **Reuses existing infrastructure** — the same Mosquitto broker and
  `paho-mqtt`/`rumqttc` publish/subscribe pattern already used elsewhere in
  this project's deployment.

### The in-process seam

Whatever arrives over MQTT lands behind a single Rust port, so `services/`
and `controller/` never touch MQTT, JSON, or any supplier detail directly —
the same seam pattern used for `SimulatorPort`, `SolverPort`, and `VtnPort`
(trait in `controller/`, concrete adapter in infra, wired at the composition
root):

```rust
// controller/weather_port.rs
#[async_trait]
pub trait WeatherForecastPort: Send + Sync {
    /// Latest known forecast. Never blocks on network I/O — reads a cached
    /// snapshot, kept fresh by a background task that owns the MQTT
    /// subscription and writes into a shared `tokio::sync::watch` channel
    /// on every inbound message.
    async fn latest(&self) -> Option<WeatherForecast>;
}
```

`services::forecast` and the planner call only `latest()` — they are
unaware of MQTT entirely. A second transport (e.g. a supplier that only
offers a polling REST API) would be a second adapter behind the same trait.

## Rust data structures

```rust
// entities/weather.rs — pure data, no I/O, no crate::profile import
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkyCondition {
    Clear, MostlyClear, PartlyCloudy, Overcast, Fog, Rain, Sleet, Snow,
    Thunderstorm, Unknown, // adapter couldn't map its supplier's code — never silently guess
}

pub struct WeatherForecastSample {
    pub valid_at: DateTime<Utc>,   // hour this sample is *for*
    pub age_h: u32,                // 0 = fact; 1..=N = hours ahead
    pub temperature_c: f64,
    pub ghi_w_m2: f64,             // Global Horizontal Irradiance, unprojected
    pub wind_speed_kmh: Option<f64>,
    pub rain_prob_pct: Option<f64>,
    pub new_snowfall_cm: Option<f64>,
    pub sky_condition: Option<SkyCondition>,
    pub irradiance_variability: Option<f64>, // 0 = uniform sky, 1 = maximally broken
}

pub struct WeatherForecast {
    pub source_id: String,   // e.g. "srf_meteo"
    pub location: GeoPosition,
    pub fetched_at: DateTime<Utc>,
    pub samples: Vec<WeatherForecastSample>, // ascending by valid_at
}

pub struct GeoPosition { pub latitude_deg: f64, pub longitude_deg: f64 }
```

`WeatherForecast::is_fresh(now, max_age)` implements the staleness policy: a
cached forecast older than `max_age` (2 hours by default — a starting value,
tune once real forecast-accuracy data exists) is rejected for planning
purposes and the planner falls back to the sin model.

## The transposition problem

The weather feed's `ghi_w_m2` is Global Horizontal Irradiance: what a flat,
horizontal sensor measures. Panels are tilted at a fixed angle and face a
fixed compass direction, so the horizontal number must be projected onto the
panel's plane before it means predicted power.

### Clear-sky index transposition (the method used)

```
clear_sky_index(t) = ghi_forecast(t) / ghi_clearsky_model(t)      // 0..~1, captures cloud cover
poa_irradiance(t)   = clear_sky_index(t) × poa_clearsky_model(t)  // same index applied to the panel's own clear-sky model
```

i.e. run the same clear-sky physics model twice at time `t`: once for a
horizontal surface (`ghi_clearsky_model(t)`, the denominator that turns the
forecast into a unitless cloud-cover ratio) and once for the tilted panel
(`poa_clearsky_model(t)`, using the panel's own incidence-angle geometry).
Multiplying the ratio by the tilted clear-sky value carries the forecast's
cloud information onto the panel's actual orientation, rather than a single
hand-tuned scale constant that would silently bake in unit conversion and
site-specific loss factors together, undocumented and unverifiable as panels
age, get cleaned, or become partially shaded.

### Inputs

- **Geo position** (`latitude_deg`, `longitude_deg`) — solar-position calc.
- **Panel orientation**: `tilt_deg` (0 = flat roof, 90 = vertical wall) and
  `azimuth_deg` (compass bearing the panel faces; 0=N, 90=E, 180=S, 270=W).
- **`rated_kwp`** — DC nameplate rating at STC (1000 W/m², 25 °C cell temp).
  Converts a unitless irradiance ratio (`poa_irradiance / 1000 W/m²`) to kW.
- **`performance_ratio`** — system losses (inverter conversion, DC wiring,
  connector/mismatch, soiling, snow): typically 0.80–0.90 for a well
  maintained residential system. Distinct from module conversion efficiency,
  which is already priced into `rated_kwp`.
- **`temp_coeff_pct_per_c`** — cell-temperature derate, typically
  `−0.35 %/°C` above 25 °C, from the module datasheet.
- **`noct_c`** — nominal operating cell temperature, feeds the NOCT model:
  `T_cell ≈ T_air + (NOCT − 20)/800 × poa_irradiance_w_m2`.
- **`ac_limit_kw`** (optional) — inverter clipping cap, mirrors
  `PvInverter::export_limit_kw`'s existing clamp for the sin-model forecast.

```rust
// entities/asset_params.rs — additive to the existing sin-model PvParams
pub struct PvArrayGeometry {
    pub location: GeoPosition,
    pub tilt_deg: f64,
    pub azimuth_deg: f64,
}

pub struct PvForecastParams {
    pub rated_kwp: f64,
    pub geometry: PvArrayGeometry,
    pub performance_ratio: f64,
    pub temp_coeff_pct_per_c: f64,
    pub noct_c: f64,
    pub ac_limit_kw: Option<f64>,
}
```

The transposition itself is pure, deterministic, domain-layer math
(`entities/solar.rs`), with no I/O and no `crate::profile` dependency:

```rust
pub fn solar_position(pos: &GeoPosition, t: DateTime<Utc>) -> SolarPosition;
pub fn poa_irradiance_w_m2(ghi_w_m2: f64, sun: &SolarPosition, panel: &PvArrayGeometry) -> f64;
pub fn cell_temperature_c(air_temp_c: f64, poa_w_m2: f64, noct_c: f64) -> f64;
pub fn forecast_ac_kw(params: &PvForecastParams, sample: &WeatherForecastSample, t: DateTime<Utc>) -> f64;
```

`forecast_ac_kw` composes: solar position → POA irradiance (clear-sky-index
method) → DC power (`poa/1000 × rated_kwp`) → cell-temp derate →
`× performance_ratio` → clamp to `ac_limit_kw` → snow-cover override (applied
last — see below).

`entities::solar::resolve_weather_pv_kw`/`weather_pv_forecast_series` are the
single entry point both consumers of this math share: `GET /weather`
(read-only diagnostic) and the planner's own PV input
(`SolveRequest.weather_pv_kw` → `run_planner` →
`controller::milp_planner::inputs::build_milp_inputs`, precedence
`pv_forecast_override` > `weather_pv_kw` > sin-model fallback) — so the two
views can never silently diverge on what a `WeatherForecast` implies for PV
output.

**Known deferred accuracy gaps** (tracked as R-53 in TECHNICAL_DEBTS.md):
horizon/shading obstructions (real rooftops rarely have an unobstructed
horizon), the Perez/HDKR diffuse-sky model (the current diffuse term is
isotropic-on-zenith-only, ignoring the panel's own tilt view factor and
ground albedo), and module degradation over years (~0.5%/year, negligible for
hourly planning).

## Sky condition and fluctuation

An hourly-average irradiance number is blind to a real distinction: an hour
of uniform thin overcast and an hour where the sun alternates with passing
clouds every few minutes can average to the same kWh, but produce wildly
different PV ramp rates. This matters for planning (battery/curtailment
margin, how much to trust a slot's forecast at face value).

`irradiance_variability` (0 = uniform sky all hour, either clear or overcast;
1 = maximally broken/alternating sky within the hour) carries this signal,
derived by each adapter from whatever sub-hourly proxy its supplier actually
exposes. The SRF Meteo adapter uses minutes of actual sunshine within the
hour (`SUN_MIN`, 0–60):

```
irradiance_variability = 1 − |2 × (SUN_MIN / 60) − 1|
```

peaked at `SUN_MIN=30` (genuinely alternating sky), zero at `SUN_MIN=0` or
`60` (uniformly overcast or clear, respectively). A supplier reporting
cloud-cover % instead applies the same peaked-at-50%-shape formula to that
percentage; a supplier with genuine minute-resolution irradiance can use the
actual coefficient of variation within the hour. `None` only when the
supplier gives no proxy for it at all — the consuming side then treats the
slot as maximum-uncertainty, never as stable.

`sky_condition` is likewise adapter-translated from the supplier's own
native icon/code into the shared `SkyCondition` vocabulary — each adapter
owns a small, private lookup table (e.g. `srf_symbol_to_sky_condition` in the
`data_acquisition` publisher), never embedded in the generic entity.

### How the planner uses it

`AssetForecast` (`entities/design_vocabulary.rs`) carries a `confidence: f64`
field; the weather-sourced PV forecast folds `irradiance_variability`
straight into it (`services/forecast.rs::slot_confidence`):

```
confidence = base_confidence(age_h) × (1.0 − irradiance_variability.unwrap_or(1.0))
```

— a slot flagged as maximally broken-sky gets a low-confidence PV forecast
without the planner needing to know anything about sunshine minutes, sky
icons, or the specific supplier.

## Snow cover — a stateful override, not a per-sample field

Snow is architecturally different from `sky_condition`/`irradiance_variability`:
those are independent facts about a single hour, but whether the panel is
producing at 14:00 depends on whether it snowed at 06:00 and hasn't melted
off yet — a running state carried forward across the forecast sequence, the
same shape as `AssetState`-style tick functions elsewhere in this codebase.

The model is near-binary, not a continuous melt curve: panels absorb more
solar heat than surrounding terrain and, once melting starts, the sheet
tends to slide off a tilted surface within an hour or two ("self-clearing").

```rust
// entities/pv_snow.rs
pub struct PvSnowParams {
    pub snowfall_trigger_cm: f64,     // e.g. 0.2 — new snowfall above this triggers full coverage
    pub clear_threshold_c: f64,       // e.g. 1.0-2.0 — temp above which a covered panel self-clears within the hour
    pub covered_output_fraction: f64, // usually 0.0
}

pub struct PvSnowState { pub covered: bool }

impl PvSnowState {
    pub fn step(self, params: &PvSnowParams, sample: &WeatherForecastSample) -> Self {
        let snowed = sample.new_snowfall_cm.unwrap_or(0.0) >= params.snowfall_trigger_cm;
        let melts = sample.temperature_c >= params.clear_threshold_c;
        Self { covered: snowed || (self.covered && !melts) }
    }
}

pub fn snow_coverage_trajectory(
    initial: PvSnowState,
    params: &PvSnowParams,
    samples: &[WeatherForecastSample],
) -> Vec<PvSnowState>;
```

`forecast_ac_kw` applies this as a final override, after the
transposition/temperature-derate math: while `covered`, output is forced to
`× covered_output_fraction` regardless of what the clear-sky/POA calculation
says.

**Known gap** (R-55): `initial` (is the panel covered *right now*, at the
start of a forecast trajectory) currently only has the forecast-only
fallback implemented — running `snow_coverage_trajectory` from the `age_h=0`
sample forward. The preferred source, a cross-check against live PV
telemetry deviation (`AssetState.power_deviation_kw`: a large sustained
negative deviation on an otherwise clear/moderate-GHI day is itself strong
evidence of snow cover), is not yet wired.

## Wire contract

Two topics — a data topic and a heartbeat topic — because an hourly-cadence
data topic alone cannot distinguish "no new forecast yet" from "the plugin
crashed an hour ago."

### Transport conventions (apply to every topic)

- **Encoding**: UTF-8 JSON, no BOM.
- **Timestamps**: RFC 3339 / ISO 8601, UTC only, always with an explicit `Z`
  suffix — never a numeric offset, never local time. A publisher whose raw
  source data carries a local-time offset (e.g. SRF's `+02:00`-style
  `local_date_time`) must convert to UTC before publishing; an unconverted
  offset is a silent off-by-one-hour bug during DST transitions, not a parse
  error the consuming side would ever catch.
- **Topic naming**: `<root>/weather/<site_id>/<subtopic>`, e.g.
  `openadr-lab/weather/zunzgen/forecast`. `<root>` and `site_id` are
  deployment-configurable (VEN's `WEATHER_MQTT_ROOT`/`WEATHER_MQTT_SITE_ID`
  env vars) so multiple sites/suppliers coexist on one broker without any
  VEN-side code change.
- **QoS**: 1 (at-least-once) on every topic.
- **Retained**: `true` on every topic — a VEN instance that (re)connects
  gets the last known state immediately.
- **Forward compatibility**: consumers ignore unknown JSON keys; producers
  never remove or repurpose a field without incrementing `schema_version`'s
  major component.
- **On fetch failure**: the plugin does not touch the retained `forecast`
  topic. Only publish `forecast` on a successful fetch; signal failure via
  the `status` topic instead.

### Topic 1 — `<root>/weather/<site_id>/forecast`

**Cadence**: once per successful fetch (hourly for an hourly-refresh
supplier like SRF Meteo); never faster than every 5 minutes even if the
publisher's own polling loop is tighter, since there is no planning benefit
to sub-5-minute weather-forecast updates.

**Schema** (JSON Schema, draft 2020-12):

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "WeatherForecastMessage",
  "type": "object",
  "required": ["schema_version", "source_id", "location", "fetched_at", "samples"],
  "properties": {
    "schema_version": { "type": "string", "pattern": "^[0-9]+\\.[0-9]+\\.[0-9]+$", "examples": ["1.0.0"] },
    "source_id": { "type": "string", "minLength": 1, "description": "e.g. \"srf_meteo\"." },
    "location": {
      "type": "object",
      "required": ["latitude_deg", "longitude_deg"],
      "properties": {
        "latitude_deg":  { "type": "number", "minimum": -90,  "maximum": 90 },
        "longitude_deg": { "type": "number", "minimum": -180, "maximum": 180 }
      },
      "additionalProperties": false
    },
    "fetched_at": { "type": "string", "format": "date-time" },
    "samples": {
      "type": "array",
      "minItems": 1,
      "items": {
        "type": "object",
        "required": ["valid_at", "age_h", "temperature_c", "ghi_w_m2"],
        "properties": {
          "valid_at": { "type": "string", "format": "date-time" },
          "age_h": { "type": "integer", "minimum": 0, "maximum": 240 },
          "temperature_c":  { "type": "number", "minimum": -60, "maximum": 60 },
          "ghi_w_m2":       { "type": "number", "minimum": 0,   "maximum": 1500 },
          "wind_speed_kmh": { "type": "number", "minimum": 0,   "maximum": 300 },
          "rain_prob_pct":  { "type": "number", "minimum": 0,   "maximum": 100 },
          "new_snowfall_cm": { "type": "number", "minimum": 0,  "maximum": 200 },
          "sky_condition": {
            "type": "string",
            "enum": ["clear", "mostly_clear", "partly_cloudy", "overcast",
                     "fog", "rain", "sleet", "snow", "thunderstorm", "unknown"]
          },
          "irradiance_variability": { "type": "number", "minimum": 0, "maximum": 1 }
        },
        "additionalProperties": true
      }
    }
  },
  "additionalProperties": true
}
```

Example message:

```json
{
  "schema_version": "1.0.0",
  "source_id": "srf_meteo",
  "location": { "latitude_deg": 47.4491, "longitude_deg": 7.8081 },
  "fetched_at": "2026-07-19T05:54:48Z",
  "samples": [
    { "valid_at": "2026-07-19T05:00:00Z", "age_h": 0, "temperature_c": 15.0,
      "ghi_w_m2": 0.0, "wind_speed_kmh": 4.0, "rain_prob_pct": 10.0,
      "sky_condition": "clear", "irradiance_variability": 0.0 },
    { "valid_at": "2026-07-19T06:00:00Z", "age_h": 1, "temperature_c": 16.0,
      "ghi_w_m2": 97.0, "wind_speed_kmh": 4.0, "rain_prob_pct": 14.0,
      "sky_condition": "partly_cloudy", "irradiance_variability": 0.6 }
  ]
}
```

### Topic 2 — `<root>/weather/<site_id>/status`

Distinguishes "no new forecast due yet" (normal — an hourly-refresh supplier
has nothing new most of the time) from "the publisher process died."

**Cadence**: on every status change and unconditionally at least every 5
minutes. Registered as the MQTT client's Last Will and Testament at connect
time (payload matching `status: "offline"` below).

**Schema**:

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
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
      "description": "ok = last fetch succeeded within cadence. stale = alive but upstream hasn't returned fresh data. error = last fetch attempt failed. offline = LWT payload."
    },
    "last_successful_fetch_at": { "type": "string", "format": "date-time" },
    "consecutive_failures": { "type": "integer", "minimum": 0 },
    "message": { "type": "string", "description": "Optional free-text detail. Never parsed programmatically." }
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
| `.../forecast` | on every successful fetch; ≥5 min apart; skip entirely on fetch failure | yes | 1 | no |
| `.../status` | on every status change, and unconditionally ≥ every 5 min | yes | 1 | yes, payload `status:"offline"` |

## VEN-side config

Broker host/port, `<root>` prefix, and `site_id` are env-var-driven
(`VEN/docker-compose.yml`, `VEN/profiles/ven-{1,2,3}.yaml`'s `weather_pv`
section), following the same pattern used for the VTN adapter's config.

## VEN UI surface

`GET /weather` (raw feed + derived per-slot PV forecast) and the VEN UI
Weather tab (`WeatherRawPanel`, `WeatherDerivedPanel` in
`VEN/ui/src/components/weather/`) satisfy the project's `ui-transparency`
rule for this feature.

## Multi-source fusion

Not supported: if more than one weather feed is ever configured for the same
site, VEN does not currently pick, prefer-freshest, or blend between them —
out of scope until a driving use case exists.
