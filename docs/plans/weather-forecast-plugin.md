# Weather Forecast Plugin — Architecture & Interface

Future vision. No implementation exists yet. VEN's current `services/forecast.rs`
only derives `AssetForecast`s from the planner's own solution (`ForecastSource::Optimization`)
and from learned heuristics (`ForecastSource::Heuristic`) — see
`VEN/src/entities/design_vocabulary.rs::ForecastSource`. There is no
`ForecastSource::Weather` yet and no port for an external weather feed. This
document defines that plugin end to end: the transport architecture, the
Rust-side data structures VEN needs, the physics that turns a raw irradiance
forecast into predicted PV power for a fixed-tilt panel, and the exact wire
format a plugin author must produce.

Source investigated (Pi4-Server, read-only): `data_acquisition` container's
`InfluxTransformers/SrfWeatherToInfluxDb.py` (still live — verified fresh
`WeatherForecast` points in InfluxDB `RawEnergy` bucket at the time of
writing), the flux scripts under `/srv/docker/influxdb/flux/`
(`PowerYieldPrediction.flux`, `WeatherForcastAdjustedToMeasurement.flux`,
`ForcastTimePosCalibration.flux`), and SRF's own commercial API
documentation (the PDF referenced in `SrfWeatherToInfluxDb.py`'s source
comment).

## Architecture: MQTT pub/sub

The plugin publishes forecast payloads to a well-known MQTT topic (e.g.
`openadr-lab/weather/<site_id>/forecast`) in a documented JSON schema. VEN
runs an MQTT client subscribed to that topic; the "plugin" is nothing more
than *any* process, anywhere on the network, that publishes to that topic
in the agreed schema — it doesn't have to be Rust, doesn't have to be
started or supervised by VEN, and doesn't even have to be a single
well-defined artifact (a Node-RED flow, a Python cron job, or a local
weather station's own MQTT firmware all qualify equally).

This matches the actual deployment shape directly: weather data suppliers
are numerous, regional, and sometimes purely local (an on-site weather
station publishing over MQTT), so the interface has to decouple VEN from
any single supplier's transport, auth model, or release cadence. MQTT gives
that for free:

- **Maximum decoupling** — VEN never spawns, restarts, or version-matches a
  plugin process. It only needs a topic name and a schema.
- **Naturally push-based**, which fits weather data well — irradiance and
  temperature changes are event-like on the timescale VEN cares about
  (hourly), not something worth a tight polling loop.
- **Retained messages** — the broker (Mosquitto, in this project's existing
  deployment) keeps the last message on a topic, so a VEN instance that
  starts up or reconnects gets the last known forecast immediately, without
  waiting for the next publish. Cheap staleness recovery for free.
- **Multiple plugins coexist** on different topics/sites without any
  VEN-side code change — new supplier, new site, new local sensor: all just
  publish to their own topic under the agreed schema.
- **Reuses existing infrastructure** — a Mosquitto broker is already part
  of this project's deployment (see the `mosquitto` project on Pi4-Server,
  already on the same docker network as the rest of the stack), and the
  stack already has a working MQTT publish pattern (`mqtt_bridge`'s
  `paho-mqtt` usage) to model a new plugin on.

The one thing MQTT doesn't give directly is synchronous "give me the
forecast right now" pull — not needed here, since retained messages already
answer "give me the latest" instantly, and the wire contract's heartbeat
topic (below) covers "is this data still trustworthy."

### The in-process seam

Whatever arrives over MQTT lands behind a single Rust port, so `services/`
and `controller/` never touch MQTT, JSON, or any supplier detail directly —
the same seam pattern already used for `SimulatorPort`, `SolverPort`, and
`VtnPort` (`VEN/src/controller/*_port.rs`: trait in `controller/`, concrete
adapter in infra, wired at the composition root):

```rust
// controller/weather_port.rs — same ring as SimulatorPort/SolverPort/VtnPort
#[async_trait]
pub trait WeatherForecastPort: Send + Sync {
    /// Latest known forecast. Never blocks on network I/O — reads a cached
    /// snapshot, kept fresh by a background task that owns the MQTT
    /// subscription and writes into a shared `tokio::sync::watch` channel
    /// on every inbound message.
    async fn latest(&self) -> Option<WeatherForecast>;
}
```

This keeps `services::forecast` and the planner unaware of MQTT entirely —
they only ever call `latest()`. If a second transport is ever justified
later (e.g. a supplier that only offers a polling REST API), it becomes a
second adapter behind the *same* trait, not a change to this one.

## Rust data structure

Extends the existing "design vocabulary" sketch pattern
(`entities/design_vocabulary.rs` already has `ForecastSource::WeatherModel`
and `ExternalDataSourceType::Weather`/`Irradiation` as unimplemented
placeholders — this is the type that fills them in).

```rust
// entities/design_vocabulary.rs (or a new entities/weather.rs — TBD at
// implementation time, size-cap dependent) — pure data, no I/O, no
// crate::profile import per the entities-layer rule.

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SkyCondition {
    Clear,
    MostlyClear,
    PartlyCloudy, // "changing" — pair with a high irradiance_variability score
    Overcast,
    Fog,
    Rain,
    Sleet,
    Snow,
    Thunderstorm,
    Unknown, // adapter couldn't map its supplier's code — never silently guess
}

/// One hour of forecast, as delivered by whichever supplier is configured.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherForecastSample {
    /// Hour this sample is *for* (not when it was fetched).
    pub valid_at: DateTime<Utc>,
    /// 0 = most recent actual/"fact", 1..=N = hours ahead. Lets the
    /// planner distinguish "current conditions" from "forecast", and lets
    /// staleness policy (mirrors `StaleRatePolicy`) reject samples whose
    /// age_h has drifted past the fetch interval without a refresh.
    pub age_h: u32,
    pub temperature_c: f64,
    /// Global Horizontal Irradiance, as reported by the supplier — NOT yet
    /// projected onto any panel plane. That projection is VEN's job (below),
    /// because it depends on VEN's own site/array parameters, which the
    /// weather supplier does not and should not know about.
    pub ghi_w_m2: f64,
    pub wind_speed_kmh: Option<f64>,
    pub rain_prob_pct: Option<f64>,
    /// New snowfall this hour. Drives the snow-cover state model below.
    pub new_snowfall_cm: Option<f64>,
    /// Supplier-specific icon/description, translated by the adapter into
    /// this shared vocabulary. Optional — many suppliers won't have one,
    /// or the adapter chooses not to map it.
    pub sky_condition: Option<SkyCondition>,
    /// 0.0 = sky was uniform the whole hour (fully clear OR fully
    /// overcast — either way, stable), 1.0 = maximally broken/alternating
    /// sky within the hour (highest expected ramp-rate risk). Each adapter
    /// derives this from whatever sub-hourly signal its supplier actually
    /// has (see "Sky condition and fluctuation" below). `None` only if the
    /// supplier gives no proxy for it at all (fall back to treating the
    /// slot as maximum-uncertainty, not as stable, at the consumption
    /// site).
    pub irradiance_variability: Option<f64>,
}

/// A full forecast pull: one fetch, many hourly samples, tied to a location.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherForecast {
    pub source_id: String, // e.g. "srf_meteo", matches ExternalDataSource::source_id
    pub location: GeoPosition,
    pub fetched_at: DateTime<Utc>,
    /// Ordered by valid_at ascending. The port makes no promise about exact
    /// length — a different supplier might give 24h or 72h instead of
    /// SRF's 48h.
    pub samples: Vec<WeatherForecastSample>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct GeoPosition {
    pub latitude_deg: f64,
    pub longitude_deg: f64,
}
```

## What the current feed gives us

The SRF Meteo API forecast is hourly, 48 hours ahead (`age_h` 1..=48), plus
one hour back (`age_h=0`, the "fact"/actual). Fields captured in Influx per
hour: `temperature_C`, `windspeed_kmh`, `rain_mmh`, `sunshine_minh`,
`dewpoint_C`, `relhumid_Percent`, `snow_cmh`, `irradiance_Wm2`,
`pressure_hPa`, `rainprob_Percent`, plus per-day `sunrise`/`sunset`/`UVI`.

For VEN's planning horizon, only two are load-bearing right now:

- **`irradiance_Wm2`** — drives PV generation forecast.
- **`temperature_C`** — drives heater/heat-pump thermal-model forecast
  (`ThermalModelParams` in `entities/design_vocabulary.rs`) and is also the
  input to the PV cell-temperature derate (see below).

Worth carrying even though nothing consumes them yet, because they're
already in the feed at zero extra cost and the planner may want them later:

- `windspeed_kmh` — cools PV modules (better yield than temperature-only
  models predict) and increases building heat loss for the thermal model.
- `rainprob_Percent` / `rain_mmh` — soiling/snow-cover proxy for PV, and a
  heuristic-override signal for outdoor-dependent loads.
- `sunrise`/`sunset` — cheap sanity bound: zero out PV forecast outside
  daylight without running the full solar-position math.

Not worth carrying yet: `dewpoint_C`, `relhumid_Percent`, `pressure_hPa`,
`UVI` — no consumer, no planned one, drop until something needs them.

## The transposition problem

SRF's `irradiance_Wm2` is (as best determined from the flux scripts and
standard practice for this kind of API) **Global Horizontal Irradiance**:
what a flat, horizontal sensor would measure. Panels are tilted at a fixed
angle and face a fixed compass direction, so the horizontal number has to
be projected onto the panel's plane before it means anything for predicted
power. This is exactly what `WeatherForcastAdjustedToMeasurement.flux` is
doing.

### The flux model, restated

1. **Solar position** `(elevation h, azimuth A)` at time `t`, from
   `latitude`/`longitude` — a standard low-precision solar-position formula
   (julian date → mean longitude/anomaly → ecliptic → right ascension/
   declination → hour angle → elevation/azimuth). Accurate to a fraction of
   a degree, plenty for hourly PV forecasting.
2. **Panel orientation as a normal vector**: same elevation/azimuth math
   applied to the panel's own normal direction (`cellElevation`,
   `cellAzimuth` in the script — note their convention: `cellElevation` is
   the *normal vector's* elevation, so a flat/horizontal panel is
   `cellElevation = 90°`, and `cellElevation = 90° − tilt_from_horizontal`
   for a tilted panel).
3. **Incidence angle**: dot product of the sun's unit vector and the
   panel's unit normal → `cos(incidence angle)`. This is the fraction of
   direct-beam irradiance that actually lands on the tilted plane instead
   of grazing it.
4. **Direct/diffuse split** (since GHI conflates both): Kasten-Young air
   mass from elevation, an empirical diffuse-fraction curve
   `Kd = 0.271 − 0.294·exp(−0.036·(90−h))`, direct-beam transmittance
   `Kn = exp(−k·AM)` with `k ≈ 0.21`. Direct component is projected through
   the incidence-angle cosine (step 3); diffuse component is treated as
   isotropic, scaled only by solar elevation (no view-factor correction for
   panel tilt — a known simplification, see below).
5. **Sum** direct + diffuse → plane-of-array irradiance estimate, scaled by
   a `roofSizeFactor` (a stand-in for kWp × some efficiency guess) to get
   watts.

This whole pipeline in step 1-5 is a **clear-sky model** — it never touches
the SRF forecast at all. Separately, the same file also reads the raw SRF
`irradiance_Wm2` and multiplies it by a hand-tuned constant
(`* 64.0 * 0.16` / `* 64.0 * 0.22` in different scripts) to compare against
real Fronius PV output. Those constants look like ad-hoc curve-fitting
against actual production, not a documented physical model — this is the
piece most worth formalizing rather than porting as-is.

### The standard, cleaner way to combine both

The textbook approach (this is what commercial PV-forecast services do,
under names like "clear-sky index transposition" or the Liu-Jordan / HDKR
model) is:

```
clear_sky_index(t) = ghi_forecast(t) / ghi_clearsky_model(t)   // 0..~1, captures cloud cover
poa_irradiance(t)   = clear_sky_index(t) × poa_clearsky_model(t)  // same index applied to the PANEL's clear-sky model, not the horizontal one
```

i.e. run the same clear-sky physics model **twice** at time `t`: once for a
horizontal surface (to get `ghi_clearsky_model(t)`, the denominator that
turns the forecast into a unitless cloud-cover ratio) and once for the
actual tilted panel (`poa_clearsky_model(t)`, using the panel's own
incidence-angle geometry from step 3 above). Multiplying the ratio by the
tilted clear-sky value carries the forecast's cloud information onto the
panel's actual orientation, instead of hand-tuning a single scale constant
that silently bakes in both the unit conversion *and* the site's specific
loss factors, undocumented and unverifiable when e.g. panels get cleaned or
partially shaded by a growing tree.

### The four transformation inputs — and what's missing

Geo-position coordinates, azimuth and elevation, installed peak power, and
PV efficiency were the starting list. Walking through what each buys, plus
the gaps:

- **Geo position** (`latitude_deg`, `longitude_deg`) — needed for the
  solar-position calculation (step 1 above). Nothing else uses it directly.
- **Azimuth and elevation** — this is the *panel's* orientation (its normal
  vector), not the sun's. Needed for the incidence-angle projection
  (step 3). Two numbers: **tilt** (angle from horizontal — 0° flat roof,
  90° vertical wall) and **azimuth** (compass bearing the panel faces —
  0°=N, 90°=E, 180°=S, 270°=W, matching the same convention the
  solar-position function already outputs, so no unit-convention mismatch
  at the call site).
- **Installed peak power** (`rated_kwp`) — the DC nameplate rating at STC
  (1000 W/m², 25 °C cell temp, per manufacturer datasheet). Converts a
  unitless irradiance ratio (`poa_irradiance / 1000 W/m²`) into kW.
- **PV efficiency** — this is the one to be careful with, because
  "efficiency" conflates two *different* numbers people often mean:
  - Module conversion efficiency (~20% for typical panels) — already priced
    into `rated_kwp`. You don't need it separately if you have the
    nameplate rating.
  - **System performance ratio** — everything *else* that reduces AC output
    below the naive `poa_irradiance/1000 × rated_kwp`: inverter conversion
    loss, DC wiring resistance, connector/mismatch losses, soiling, snow
    cover. Typically 0.80–0.90 for a well-maintained residential system.
    This is what the flux script's magic constant is really trying to
    capture, undocumented.

**What's missing from the original four, that a real forecast needs**:

1. **Cell-temperature derate** — PV output drops roughly `−0.35 %/°C` above
   25 °C cell temperature (typical crystalline-silicon coefficient from the
   module datasheet). Cell temp itself is estimated from ambient air temp +
   irradiance via the NOCT model: `T_cell ≈ T_air + (NOCT − 20)/800 × poa_irradiance_w_m2`.
   This is exactly why the weather feed's `temperature_C` matters for PV,
   not just for the heater thermal model.
2. **Direct/diffuse split accuracy** — the flux model's diffuse term is
   isotropic-on-zenith-only; it ignores the panel's own tilt view factor
   (a tilted panel "sees" less of a uniformly diffuse sky and some
   ground-reflected light) and ground albedo. Fine for a first cut; the
   Perez or HDKR diffuse models are the standard upgrade if accuracy ever
   matters more than it does today.
3. **Horizon/shading obstructions** — trees, chimneys, neighboring
   buildings. Pure geometry (steps 1-4) assumes an unobstructed horizon;
   real rooftops rarely are. Usually modeled as a per-azimuth horizon
   profile (a lookup table of "minimum sun elevation visible at this
   compass bearing") multiplied in as a binary or soft cutoff. Not
   essential for v1 — flagging as a known accuracy ceiling.
4. **AC/inverter clipping cap** — if the inverter's AC rating is below the
   DC `rated_kwp` (common, intentional oversizing), forecast power must be
   clamped to that AC limit on sunny midday hours. `PvInverter` in
   `assets/pv.rs` already has this concept (`export_limit_kw`) for the
   simulator's sin-model forecast — the real-weather path should reuse the
   same clamp.
5. **Module degradation over years** — negligible (~0.5%/year) for hourly
   planning; not worth modeling now, mention only for completeness.

### Where this plugs into VEN

`entities/asset_params.rs::PvParams` currently has only `rated_kw` and a
sin-model `forecast_kw(ts)` (used by the simulator, not by any real weather
feed). The real-weather path is a **parallel, additive** capability — it
doesn't replace the sin model (still useful for the simulator/demo mode
without a live weather feed) — introduced as a second, richer params type:

```rust
// entities/asset_params.rs — new type, PvParams unchanged
pub struct PvArrayGeometry {
    pub location: GeoPosition,
    pub tilt_deg: f64,     // 0 = horizontal, 90 = vertical wall
    pub azimuth_deg: f64,  // compass bearing the panel faces (0=N,90=E,180=S,270=W)
}

pub struct PvForecastParams {
    pub rated_kwp: f64,
    pub geometry: PvArrayGeometry,
    pub performance_ratio: f64,     // system losses: inverter+wiring+soiling+mismatch (e.g. 0.87)
    pub temp_coeff_pct_per_c: f64,  // e.g. -0.35, applied above 25 °C cell temp
    pub noct_c: f64,                // nominal operating cell temp for the NOCT model, e.g. 45.0
    pub ac_limit_kw: Option<f64>,   // inverter clipping cap, mirrors PvInverter::export_limit_kw
}
```

The transposition itself is pure math (no I/O, no `crate::profile`
dependency) — deterministic function of `(WeatherForecastSample,
PvArrayGeometry, DateTime<Utc>)` — so it belongs in the domain layer next to
`PvParams::forecast_kw`, fully unit-testable without a network or a clock
mock beyond the existing injectable-clock convention:

```rust
pub fn solar_position(pos: &GeoPosition, t: DateTime<Utc>) -> SolarPosition; // { elevation_deg, azimuth_deg }
pub fn poa_irradiance_w_m2(ghi_w_m2: f64, sun: &SolarPosition, panel: &PvArrayGeometry) -> f64;
pub fn cell_temperature_c(air_temp_c: f64, poa_w_m2: f64, noct_c: f64) -> f64;
pub fn forecast_ac_kw(params: &PvForecastParams, sample: &WeatherForecastSample, t: DateTime<Utc>) -> f64;
```

`forecast_ac_kw` composes the above: solar position → POA irradiance (via
the clear-sky-index method, once a clear-sky model function exists for both
horizontal and panel-plane) → DC power (`poa/1000 × rated_kwp`) → cell-temp
derate → `× performance_ratio` → clamp to `ac_limit_kw` → snow-cover
override (see below, applied last).

## Sky condition and fluctuation

An hourly-average irradiance number is blind to a real distinction: an hour
of uniform thin overcast and an hour where the sun alternates with passing
clouds every few minutes can average to the *same* kWh, but produce
wildly different PV ramp rates — which matters directly for planning
(battery/curtailment margin, whether to trust a slot's forecast at face
value). This is the "if partly cloudy, expect strong fluctuation" intuition
— it needs its own signal, separate from the mean.

Checked the actual SRF hourly payload (not just the schema) for what's
available beyond irradiance/temperature:

```
2024-06-26T08:00  SUN_MIN=19  SYMBOL=4  IRR=97    FF=2  FX=6
2024-06-26T09:00  SUN_MIN=20  SYMBOL=4  IRR=181   FF=2  FX=6
2024-06-26T10:00  SUN_MIN=18  SYMBOL=3  IRR=283   FF=2  FX=7
2024-06-26T12:00  SUN_MIN=21  SYMBOL=3  IRR=456   FF=4  FX=9
```

Two fields carry the volatility signal that plain `irradiance_Wm2` doesn't:

- **`SUN_MIN`** — minutes of actual sunshine within the 60-minute hour
  (0–60). This is a *direct, numeric, intra-hour* clear/cloudy fraction —
  `SUN_MIN=60` means the sun was unobstructed the whole hour (stable,
  whatever the level), `SUN_MIN=0` means fully overcast (also stable, just
  low), and `SUN_MIN` near the middle of the range (20–40) means the sky
  was genuinely broken within that hour — sun and cloud alternating — which
  is exactly the "partly cloudy → fluctuation" case. It's continuous, so no
  lookup table is needed, and most weather APIs expose an equivalent
  (`sunshine_duration`, or a `cloud_cover_pct` that gives the same
  reasoning inverted: variability peaks in the *middle* of the 0–100% band,
  not at the extremes).
- **`SYMBOL_CODE`** — an icon/description code. Checked SRF's own commercial
  API documentation for the actual legend rather than guessing from
  observed values. Structure: **the sign is a day/night rendering hint, the
  magnitude is the actual weather condition** — negative codes mirror
  positive ones one-for-one (`19` and `-19` are both "bedeckt"/overcast),
  except the clear-sky codes get different words at night since "sunny"
  doesn't apply after dark (`1` sonnig/sunny → `-1` klar/clear; `10`
  ziemlich sonnig → `-10` klare Abschnitte/clear periods). VEN has no use
  for the day/night rendering distinction (solar elevation already tells it
  that), so an adapter should map on `code.abs()` and discard the sign
  entirely:

  | \|code\| | SRF description | generic category |
  |---|---|---|
  | 1 | sonnig (sunny) | `Clear` |
  | 10 | ziemlich sonnig (fairly sunny) | `MostlyClear` |
  | 3 | **teils sonnig (partly sunny)** | `PartlyCloudy` |
  | 19 | bedeckt (overcast) | `Overcast` |
  | 20 | regnerisch / bewölkt: etwas Regen | `Rain` |
  | 4, 25 | Regenschauer (rain showers) | `Rain` |
  | 5 | Regenschauer mit Gewitter | `Thunderstorm` |
  | 21 | Schneefall (snowfall) | `Snow` |
  | 6 | Schneeschauer (snow showers) | `Snow` |
  | 22 | Schneeregen (sleet) | `Sleet` |
  | 8 | Schneeregenschauer (sleet showers) | `Sleet` |
  | 2 | Nebelbänke (fog banks) | `Fog` |
  | 17 | Nebel (fog) | `Fog` |

  SRF's own word for code `3` is literally **"teils sonnig" — partly
  sunny** — which is exactly the "changing" condition this whole section is
  about, and confirms `PartlyCloudy` is the right bucket to pair with a
  high `irradiance_variability` score, straight from the vendor's own
  vocabulary rather than an assumption.

  Every supplier has its *own* icon set, so this field is the one place
  where some normalization is unavoidable if the interface is to stay
  supplier-agnostic (unlike the OpenADR VTN pass-through convention
  elsewhere in this project, there is no single spec to pass through here —
  the whole point of this port is to abstract over many incompatible
  suppliers, so a small shared vocabulary is the actual value of the
  abstraction, not avoidable boilerplate). Each adapter owns a small,
  private lookup table like the one above, translating its supplier's
  native code into the shared `SkyCondition` enum — the table lives with
  the adapter, never in the generic entity:

  ```rust
  // adapter-private, e.g. infra/weather/srf_meteo.rs — NOT in entities/
  fn srf_symbol_to_sky_condition(code: i32) -> SkyCondition {
      match code.abs() {
          1 => SkyCondition::Clear,
          10 => SkyCondition::MostlyClear,
          3 => SkyCondition::PartlyCloudy,
          19 => SkyCondition::Overcast,
          20 | 4 | 25 => SkyCondition::Rain,
          5 => SkyCondition::Thunderstorm,
          21 | 6 => SkyCondition::Snow,
          22 | 8 => SkyCondition::Sleet,
          2 | 17 => SkyCondition::Fog,
          _ => SkyCondition::Unknown, // never silently guess a new/unmapped code
      }
  }
  ```
- Secondary corroborating signal: **gust vs. steady wind** (`FX_KMH` vs
  `FF_KMH`) — a widening gust/steady ratio often co-occurs with the kind of
  atmospheric instability that produces broken-cloud conditions. Not worth
  a dedicated field; useful only as a cross-check if `SUN_MIN`/cloud-cover
  data is ever missing from a given supplier.

Each adapter derives `irradiance_variability` from whatever sub-hourly
signal its supplier actually has — SRF: `1 - |2×(SUN_MIN/60) - 1|`; a
supplier that instead gives cloud-cover %: the same peaked-at-50%-shape
formula applied to that percentage; a supplier with genuine
minute-resolution irradiance: the actual coefficient of variation within
the hour, which is strictly better data if available. `None` only if the
supplier gives no proxy for it at all (fall back to treating the slot as
maximum-uncertainty, not as stable, at the consumption site).

### How the planner would use it

No new consumer needed — `AssetForecast` (`entities/design_vocabulary.rs`)
already carries a `confidence: f64` field for exactly this purpose. The PV
forecast-building step (the future counterpart to
`services::forecast::build_asset_forecasts`) folds
`irradiance_variability` straight into that existing field instead of
adding a parallel one:

```
confidence = base_confidence(age_h) × (1.0 − irradiance_variability.unwrap_or(1.0))
```

— a slot flagged as maximally broken-sky gets a low-confidence PV forecast
without the planner needing to know anything about sunshine minutes, sky
icons, or SRF at all. This is the same reason the port/entity split
matters: the *volatility* concept is generic and belongs in the shared
struct; the *translation* from any one supplier's raw fields into it is
adapter-only code, isolated behind the port.

## Snow cover — a stateful override, not a per-sample field

Snow is architecturally different from everything above. `sky_condition`
and `irradiance_variability` are independent facts about a single hour —
each sample stands on its own. Snow cover doesn't: whether the panel is
producing at 14:00 depends on whether it snowed at 06:00 and hasn't melted
off yet, which the 14:00 sample alone can't tell you. It needs a **running
state carried forward across the forecast sequence**, the same shape as
`AssetState`/`PvState` already carry `soc`/`actual_power_kw` tick-to-tick —
not a fact attached to one sample.

The raw ingredient is already there: SRF's hourly payload has
`FRESHSNOW_CM` (verified non-zero in winter months) already mapped to
`snow_cmh` in the Influx schema. Generalizes cleanly:
`new_snowfall_cm: Option<f64>` on `WeatherForecastSample` (already included
above) — most weather APIs report snowfall, so this is still a generic
field, not an SRF-specific one.

### The model: near-binary, not a continuous melt curve

The real-world behavior of snow on a **tilted, dark PV panel** is not a
slow linear melt like snow on flat ground — panels absorb more solar heat
than the surrounding terrain and, once the surface layer starts melting,
the whole sheet tends to slide off the tilted surface within an hour or two
("self-clearing"). So a two-state model tracks reality closer than a
continuous depth integrator, and is far simpler to reason about for
planning purposes:

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PvSnowParams {
    /// New snowfall this hour above this amount triggers full coverage.
    /// A light dusting still blocks most direct irradiance, so keep this
    /// small (e.g. 0.2 cm), not zero (avoid flagging on measurement noise).
    pub snowfall_trigger_cm: f64,
    /// Temperature above which a covered panel is assumed to self-clear
    /// within the hour (tilt + dark-surface absorption melt panels faster
    /// than ambient air temperature alone would suggest — this threshold
    /// is deliberately close to 0°C, not well above it, e.g. 1.0-2.0°C).
    pub clear_threshold_c: f64,
    /// Output fraction while covered — usually 0.0, but a steep-tilt panel
    /// may still get a sliver of edge light; 0.0 is the safe default.
    pub covered_output_fraction: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PvSnowState {
    pub covered: bool,
}

impl PvSnowState {
    /// Pure state transition for one forecast hour — same shape as
    /// AssetState-style tick functions elsewhere: (old_state, inputs) → new_state.
    pub fn step(self, params: &PvSnowParams, sample: &WeatherForecastSample) -> Self {
        let snowed = sample.new_snowfall_cm.unwrap_or(0.0) >= params.snowfall_trigger_cm;
        let melts = sample.temperature_c >= params.clear_threshold_c;
        Self {
            covered: snowed || (self.covered && !melts),
        }
    }
}
```

### Running it forward over the forecast

Because the planner needs the *whole* horizon's trajectory in one shot
(not a live tick-by-tick simulation), this is a pure fold over
`WeatherForecast::samples`, starting from a known/assumed current state —
the same "pure function over a time series" shape as
`PvInverter::build_milp_context` and `AssetHeuristics::sample_kw` already
use elsewhere in this codebase:

```rust
pub fn snow_coverage_trajectory(
    initial: PvSnowState,
    params: &PvSnowParams,
    samples: &[WeatherForecastSample],
) -> Vec<PvSnowState> {
    let mut state = initial;
    samples
        .iter()
        .map(|s| {
            state = state.step(params, s);
            state
        })
        .collect()
}
```

`forecast_ac_kw` applies this as a final override, after the
transposition/temperature-derate math, not instead of it: while `covered`,
output is forced to `× covered_output_fraction` regardless of what the
clear-sky/POA calculation says — snow cover dominates irradiance entirely,
so it must be the last multiplier applied, not blended in.

### The open problem: where does `initial` come from?

The forecast alone can drive the trajectory forward, but it needs a
starting truth — "is the panel covered *right now*?" — which is a physical
fact the weather forecast doesn't know. Two sources, in order of
preference:

1. **Real telemetry, if available**: the existing `AssetState.power_deviation_kw`
   concept (`actual_kw − commanded_kw`) already tracks live vs. expected
   deviation — a large, sustained negative deviation on an otherwise
   clear/moderate-GHI day (real Fronius output near zero when the
   clear-sky model says it shouldn't be) is itself strong direct evidence
   of snow cover, independent of any weather forecast. This is the
   trustworthy source when a live PV reading exists.
2. **Forecast-only fallback**: run `snow_coverage_trajectory` starting from
   `age_h=0` (the "fact" hour, current actual conditions) using only the
   forecast's own recent-past samples to infer the current state when no
   live telemetry cross-check is wired up yet (e.g. simulator/demo mode,
   or a newly onboarded site with no PV history).

Both paths converge on the same `PvSnowState` shape, so the choice of
which one feeds `initial` is a wiring decision at the composition root, not
a fork in the domain model.

## Wire contract

Everything above defines the Rust-side shape. This section defines the
**wire format** — what a plugin publisher on the MQTT broker must actually
send, byte for byte, so that a plugin author who has never seen this
codebase (and a VEN maintainer who has never seen a given plugin) can both
implement against this document alone and interoperate correctly. Two
topics, not one — a data topic and a heartbeat topic — because an
hourly-cadence data topic alone cannot distinguish "no new forecast yet"
from "the plugin crashed an hour ago."

### Transport conventions (apply to every topic below)

- **Encoding**: UTF-8 JSON, no BOM, no trailing newline required.
- **Timestamps**: RFC 3339 / ISO 8601, UTC only, always with an explicit
  `Z` suffix — never a numeric offset, never local time. SRF's own raw
  `local_date_time` field carries a `+02:00`-style offset; the *adapter*
  must convert to UTC before publishing. This is a hard requirement, not a
  style preference: `entities::WeatherForecastSample.valid_at` is a
  `chrono::DateTime<Utc>` on the consuming side, and an ambiguous or
  local-offset timestamp is a silent correctness bug (off-by-one-hour
  during DST transitions) rather than a parse error.
- **Topic naming**: `<root>/weather/<site_id>/<subtopic>`, e.g.
  `openadr-lab/weather/main-roof/forecast`. `site_id` is a free-form slug
  chosen by whoever deploys the plugin — it exists purely so multiple
  sites/arrays can coexist on one broker without any VEN-side code change.
  `<root>` is deployment-configurable (not fixed to `openadr-lab`) so an
  existing MQTT namespace can host this without collision.
- **QoS**: 1 (at-least-once) on every topic. QoS 2 is unnecessary overhead
  — a duplicate delivery of the same forecast is harmless because the
  consumer keys on `fetched_at`, not on message identity.
- **Retained**: `true` on every topic. A VEN instance that starts up or
  reconnects gets the last known state immediately, without waiting for
  the next publish — the cheap staleness-recovery property MQTT gives for
  free.
- **Forward compatibility**: consumers must ignore unknown JSON object
  keys rather than reject the message. Producers must not remove or
  repurpose a field without incrementing `schema_version`'s major
  component. This is the only compatibility rule that matters in practice
  — it lets a plugin add a new optional field (e.g. a future supplier's
  humidity-based icing risk) without a coordinated flag-day upgrade of
  every VEN instance subscribed to it.
- **On fetch failure**: the plugin must **not** touch the retained
  `forecast` topic. Publishing stale data relabeled with a fresh
  `fetched_at` would make VEN trust data that didn't actually update; the
  correct signal for "something's wrong" is the `status` topic below, left
  independent so a consumer can always tell "old but honestly stale" from
  "silently re-stamped."

### Topic 1 — `<root>/weather/<site_id>/forecast`

**Cadence**: published once per successful fetch. For an hourly-refresh
supplier like SRF, that's hourly; a supplier with a faster native refresh
may publish more often, but a plugin should not publish faster than every
5 minutes even if its own polling loop is tighter than that — there is no
planning benefit to sub-5-minute weather-forecast updates, and it wastes
retained-message churn on the broker. Always publish on every successful
fetch, whether or not the values actually changed since the last one —
this keeps "is this fresh" a pure function of `fetched_at`, never a content
diff the consumer has to reason about.

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
      "description": "Semver of this message shape. Consumers reject a message whose MAJOR component they don't understand; minor/patch bumps are always additive.",
      "pattern": "^[0-9]+\\.[0-9]+\\.[0-9]+$",
      "examples": ["1.0.0"]
    },
    "source_id": {
      "type": "string",
      "description": "Identifies the supplier/plugin, e.g. \"srf_meteo\". Free text — not an enum, since new suppliers must not require a schema change.",
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
      "description": "UTC timestamp of the upstream API call this message reports. Not the MQTT publish time — a retry that re-sends the same fetch must keep the original fetched_at."
    },
    "samples": {
      "type": "array",
      "minItems": 1,
      "description": "Ascending by valid_at. No fixed length — a 48-hour supplier and a 24-hour supplier both produce a valid message; the consumer must not assume a specific count.",
      "items": {
        "type": "object",
        "required": ["valid_at", "age_h", "temperature_c", "ghi_w_m2"],
        "properties": {
          "valid_at": {
            "type": "string",
            "format": "date-time",
            "description": "UTC hour this sample forecasts (or reports, for age_h=0)."
          },
          "age_h": {
            "type": "integer",
            "minimum": 0,
            "maximum": 240,
            "description": "0 = most recent actual/\"fact\"; 1+ = hours ahead. Upper bound is a sanity cap, not an expected value — do not assume every supplier reaches 48."
          },
          "temperature_c":  { "type": "number", "minimum": -60, "maximum": 60 },
          "ghi_w_m2":       { "type": "number", "minimum": 0,   "maximum": 1500,
            "description": "Global Horizontal Irradiance, unprojected. VEN performs the panel-plane transposition; the plugin must not." },
          "wind_speed_kmh": { "type": "number", "minimum": 0,   "maximum": 300 },
          "rain_prob_pct":  { "type": "number", "minimum": 0,   "maximum": 100 },
          "new_snowfall_cm": { "type": "number", "minimum": 0,  "maximum": 200 },
          "sky_condition": {
            "type": "string",
            "enum": ["clear", "mostly_clear", "partly_cloudy", "overcast",
                     "fog", "rain", "sleet", "snow", "thunderstorm", "unknown"],
            "description": "Adapter-translated from the supplier's native icon/code — see the SkyCondition mapping table above. Use \"unknown\" for an unmapped native code; never omit the field to mean the same thing (omission means the supplier has no equivalent concept at all)."
          },
          "irradiance_variability": {
            "type": "number", "minimum": 0, "maximum": 1,
            "description": "0 = uniform sky for the whole hour (clear or overcast), 1 = maximally broken/alternating sky within the hour. See the sky-condition section above for how each supplier derives this."
          }
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

Exists because topic 1 alone cannot distinguish "no new forecast is due
yet" from "the plugin died an hour ago" — a fixed-cadence heartbeat is the
only way to bound that detection time independent of the data topic's own
cadence.

**Cadence**: published on every status change **and** unconditionally at
least once every 5 minutes, whether or not anything changed — a consumer
that has seen no message on this topic (retained or live) for longer than
~2× that interval should treat the plugin as dead. In addition, plugins
should register this topic as their MQTT **Last Will and Testament**
(broker-published automatically on ungraceful disconnect) with a payload
matching `status: "offline"` below — this catches a crashed process
immediately instead of waiting out the heartbeat window.

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
    "ts": { "type": "string", "format": "date-time", "description": "UTC time this status message was generated." },
    "status": {
      "type": "string",
      "enum": ["ok", "stale", "error", "offline"],
      "description": "ok = last fetch succeeded within the expected cadence window. stale = plugin alive but upstream hasn't returned fresh data within the expected window (e.g. supplier outage). error = last fetch attempt failed. offline = LWT payload, published by the broker on ungraceful disconnect."
    },
    "last_successful_fetch_at": { "type": "string", "format": "date-time" },
    "consecutive_failures": { "type": "integer", "minimum": 0 },
    "message": { "type": "string", "description": "Optional free-text detail for error/stale, e.g. \"upstream HTTP 503\". Never parsed programmatically — for humans reading the broker, not for VEN logic." }
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

## Open questions for later

- Staleness policy: how old can a cached forecast be before the planner
  should stop trusting it (mirrors `rate_estimated`/stale-rate handling
  already in `controller/milp_planner/stale_rates.rs` for tariffs)?
- Multi-source fusion: if both an MQTT local station and a cloud API are
  configured for the same site, does VEN pick one, prefer freshest, or
  blend?
- Broker security: the existing Mosquitto deployment on Pi4-Server allows
  anonymous connections on its plaintext listener; a production weather
  topic likely wants at least a password-protected or TLS listener
  (the broker already has an 8883/TLS listener configured for another
  integration) before exposing it beyond the local network.
