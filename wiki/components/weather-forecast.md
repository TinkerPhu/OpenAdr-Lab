---
title: Weather Forecast Plugin
type: component
created: 2026-07-28
updated: 2026-07-31
synced_commit: e9f5207
sources: [docs/architecture/weather_forecast.md, VEN/src/weather.rs, VEN/src/entities/weather.rs, VEN/src/entities/solar.rs, VEN/src/entities/pv_snow.rs, VEN/src/controller/weather_port.rs, VEN/src/routes/weather.rs, VEN/src/profile/weather_pv.rs, VEN/src/services/forecast.rs, VEN/ui/src/pages/Weather.tsx, VEN/src/services/test_support/mock_weather_port.rs]
tags: [weather, pv, forecast, mqtt, ven]
---

# Weather Forecast Plugin

VEN ingests an external weather forecast over MQTT (topic
`<root>/weather/<site_id>/forecast`, retained JSON) and converts it into a physics-based PV
generation forecast — an alternative to the sin-model fallback used when no feed is
configured. Full architecture (transport, transposition physics, sky-condition/snow model,
wire contract): `docs/architecture/weather_forecast.md`. This page covers what that doc
doesn't: how the feed is actually wired into the running system and where it sits relative
to the rest of the [[ven-hexagonal-architecture]].

## Port and adapter

`WeatherForecastPort` (`controller/weather_port.rs`) is the seam — `services/` and
`controller/` never see MQTT/JSON directly, mirroring `SimulatorPort`/`SolverPort`/`VtnPort`.
`VEN/src/weather.rs`'s `MqttWeatherAdapter` is the concrete implementation, subscribed via
`paho-mqtt`/`rumqttc`. Any process publishing the documented schema to the topic counts as a
"plugin" — decoupled from VEN's process lifecycle entirely (retained messages mean a
reconnecting VEN gets the last forecast immediately). The production feed for ven-1/2/3 is
the Zunzgen site, published by a separate `data_acquisition` project from SRF Meteo data.

## Two consumers, one resolution path

Both the planner and the live simulator resolve PV power through the same function,
`entities::solar::resolve_weather_pv_kw` (weather-sourced when fresh, sin-model fallback
otherwise) — this is what stops the two views from silently diverging (closed as
TECHNICAL_DEBTS.md R-50):

- **Planner input**: `SolveRequest.weather_pv_kw` → `run_planner` → `inputs::build_milp_inputs`,
  precedence `pv_forecast_override` > `weather_pv_kw` > sin-model. See [[milp-planner]].
- **Simulator ground truth**: `PvInverter.weather_power_kw`, resolved once per tick in
  `tasks::sim_tick::tick_once` and used by `step_inner` in place of the sin model —
  precedence manual sim-inject override > weather > sin model, mirroring the planner's own
  precedence. This closed the gap where the weather feed only affected `/weather`
  diagnostics and the plan, not what the simulator actually produced.
- **API-visible path**: `services::forecast::build_weather_pv_forecast`, tagged
  `ForecastSource::WeatherModel`, feeds `GET /weather` and the VEN UI Weather tab
  (`WeatherRawPanel`/`WeatherDerivedPanel`) — the required UI surface for this feed per the
  `ui-transparency` rule (`.claude/CLAUDE.md`).

`services/forecast.rs::slot_confidence` decays confidence linearly to a 0.2 floor at 48 h age
(`base_confidence(age_h)`) — a starting default, not yet tuned against real forecast-accuracy
data.

## Calibration is per-VEN and empirical

`profile/weather_pv.rs`'s `weather_pv` YAML section (`VEN/profiles/ven-{1,2,3}.yaml`) sets
`rated_kwp` and `performance_ratio` per site. These were fit against two real observed
InfluxDB irradiance/power peaks, not left at spec defaults — ven-1's 14.4 kWp reflects a real
installation, ven-2/ven-3 are hypothetical (12 kWp / 6→8 kWp), and `performance_ratio` moved
0.87 → 0.84 during calibration. A future PV site added to this lab needs the same manual
calibration pass, not just a config value copied from an existing VEN.

## Source liveness (R-52, resolved 2026-07-30)

`MqttWeatherAdapter::is_alive()` existed but was unreachable dead code — wired through
`WeatherForecastPort` (all three implementors, including the test mock), surfaced as
`source_alive` on `GET /weather` (distinct from `is_fresh`: transport health vs. content age —
a broker connection can be up with a stale retained message, or vice versa), and given a visible
chip on the VEN UI Weather page, per the `ui-transparency` rule. E2E coverage for this
specific behaviour (R-56, resolved): `weather_forecast.feature` gained a scenario publishing a
status-topic heartbeat and asserting `source_alive` flips `false`→`true`, mirroring the R-52
unit test at BDD level. Its first real Pi4 run caught a step-decorator bug (the heartbeat-publish
step was `@given` instead of `@when`, leaving it and the following assertion undefined) — fixed,
then reverified green; a pre-existing, unrelated intermittent flake in
`timeline_grid.feature` (R-61) surfaced incidentally during that same verification run.

## Known deferred gaps

`docs/reference/TECHNICAL_DEBTS.md` R-53..R-55: horizon/shading obstructions and the Perez/HDKR
diffuse-sky model are deliberately deferred accuracy improvements over the current
isotropic-on-zenith transposition; the snow-cover model's initial state has no cross-check
against live PV telemetry deviation; and the Mosquitto broker accepts anonymous publishes on
its plaintext listener (acceptable on the trusted lab LAN, revisit before any wider exposure).

## Relationship to the deviation arbiter

The weather forecast closes the "plan doesn't model PV" precondition [[deviation-arbiter]]'s
design once depended on — the plan's marginal-cost duals (`docs/architecture/ven_milp_planner.md`
§9) are only meaningful once PV forecast error stops dominating the deviation signal. Whether the
forecast measurably reduces PV-driven deviation/absorption events is still an open, unquantified
question.
