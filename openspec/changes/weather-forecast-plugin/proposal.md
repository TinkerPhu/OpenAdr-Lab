## Why

VEN has no external weather feed today. PV generation is forecast either
by the planner's own MILP solution after the fact
(`ForecastSource::Optimization`) or by learned historical heuristics
(`ForecastSource::Heuristic`) — see `VEN/src/services/forecast.rs` and
`VEN/src/entities/design_vocabulary.rs::ForecastSource`. Neither uses
actual weather data, so PV forecasts are blind to tomorrow's cloud cover,
snowfall, or a cold front, even though the planning horizon (currently
driven by tariff/rate data alone) would materially benefit from knowing
irradiance and temperature ahead of time. Weather suppliers are numerous,
regional, and sometimes purely local (an on-site weather station over
MQTT), so the feed needs to be a loosely-coupled plugin rather than a
single hard-coded API integration.

The full design (transport architecture, data structures, PV transposition
physics, sky-condition/variability signal, snow-cover model, and the exact
MQTT wire contract) is already written up in `docs/plans/weather-forecast-plugin.md`,
with the phased build order in `docs/plans/weather-forecast-implementation-plan.md`.
This proposal turns that design into an OpenSpec change so it can be
reviewed and implemented in tracked increments.

## What Changes

- Add a `WeatherForecastPort` trait (`controller/weather_port.rs`) — the
  in-process seam between VEN and any weather source, mirroring the
  existing `VtnPort`/`SolverPort`/`SimulatorPort` pattern.
- Add the domain data structures: `WeatherForecast`, `WeatherForecastSample`,
  `SkyCondition`, `GeoPosition` (entities layer, no I/O).
- Add an MQTT adapter (`VEN/src/weather.rs`) implementing the port —
  subscribes to a documented two-topic contract (`.../forecast`,
  `.../status`), parses and validates incoming JSON, never crashes on a
  malformed producer message, exposes the latest known forecast via a
  `tokio::sync::watch` channel.
- Add PV geometry/physics: `PvArrayGeometry`, `PvForecastParams`
  (`entities/asset_params.rs`), and pure transposition functions
  (`solar_position`, `poa_irradiance_w_m2`, `cell_temperature_c`,
  `forecast_ac_kw`) that turn a raw horizontal irradiance forecast into
  predicted AC power for a fixed-tilt panel, via the clear-sky-index
  transposition method.
- Add a snow-cover state model (`PvSnowParams`, `PvSnowState`,
  `snow_coverage_trajectory`) — a near-binary self-clearing model that
  overrides PV forecast to (near-)zero while snow-covered, driven by
  forecast snowfall/temperature.
- Wire the weather-sourced forecast into two places: the planner's own PV
  input (`PvInverter::build_milp_context`, replacing the sin-model
  assumption when a fresh forecast is available) and the API-visible
  forecast (`services::forecast`, tagged `ForecastSource::WeatherModel`,
  confidence derived from `irradiance_variability`) — both with the
  existing sin-model behavior kept as the fallback when no weather feed is
  configured or the cached forecast has gone stale.
- Add a staleness policy for cached weather data before it's trusted by
  the planner.
- No **BREAKING** changes — this is a new, additive capability. Existing
  simulator/demo behavior (the sin-model PV forecast) is unchanged when no
  weather plugin is configured.

## Capabilities

### New Capabilities
- `weather-forecast-ingestion`: the `WeatherForecastPort` trait, the
  domain data structures, the MQTT adapter, the wire contract (topics,
  message schemas, cadence, retained/QoS/LWT semantics), and the
  staleness policy that gates whether a cached forecast is trusted.
- `pv-weather-forecast`: the PV array geometry parameters, the
  clear-sky-index transposition physics (solar position → POA irradiance
  → cell-temperature derate → performance ratio → AC clamp), the
  sky-condition/irradiance-variability signal and how it feeds
  `AssetForecast.confidence`, and the snow-cover override model.

### Modified Capabilities
(none — `ForecastSource::WeatherModel` and
`ExternalDataSourceType::Weather`/`Irradiation` already exist as
unimplemented placeholders in `entities/design_vocabulary.rs`; this change
fills them in rather than altering any shipped requirement.)

## Impact

- **Affected service**: VEN only (Rust/Tokio). No VTN, BFF, VEN UI, or VTN
  UI changes.
- **New dependency**: an MQTT client crate (`rumqttc`), pinned per the
  project's `dependencies` rule; audited for license and `cargo audit`
  before merge.
- **New infrastructure dependency**: an MQTT broker reachable from VEN
  (the project's existing Mosquitto deployment on Pi4-Server already
  satisfies this — no new broker needs to be stood up).
- **Affected files**: see the File Layout table in
  `docs/plans/weather-forecast-implementation-plan.md` — spans
  `entities/`, `controller/`, a new top-level `weather.rs` adapter,
  `assets/pv.rs`, `controller/milp_planner/`, `services/forecast.rs`,
  `main.rs` (composition root), plus BDD coverage under `tests/features/`.
- **No openleadr-rs change required** — this is entirely internal to VEN's
  own forecasting; it doesn't touch OpenADR program/event/report handling.
- **No OpenADR 3.1 spec constraint applies** — weather data is not part of
  the OpenADR wire protocol; this is a VEN-internal planning input.

## Non-goals

- Multi-source fusion (more than one weather feed configured for the same
  site).
- Broker security hardening (TLS, auth) beyond what the MQTT adapter needs
  to function against the existing broker.
- Telemetry-based cross-check for the snow-cover model's initial state —
  ships with the forecast-only fallback only.
- Horizon/shading obstruction modeling, Perez/HDKR diffuse-sky model,
  module degradation over time — recorded as technical debt, not built.
- Any cloud/REST-polling weather supplier — MQTT is the only transport
  this change implements; a second adapter behind the same port is future
  work, not part of this change.
