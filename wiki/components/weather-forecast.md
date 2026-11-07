---
title: Weather Forecast Plugin
type: component
created: 2026-07-28
updated: 2026-10-31
synced_commit: 102f7fa
sources: [docs/architecture/weather_forecast.md, VEN/src/tasks/sim_tick/feeds.rs, VEN/src/weather.rs, VEN/src/entities/weather.rs, VEN/src/entities/solar.rs, VEN/src/entities/pv_snow.rs, VEN/src/controller/weather_port.rs, VEN/src/routes/weather.rs, VEN/src/profile/weather_pv.rs, VEN/src/services/forecast.rs, VEN/ui/src/pages/Weather.tsx, VEN/src/services/test_support/mock_weather_port.rs]
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
`controller/` never see MQTT/JSON directly, mirroring `SolverPort`/`VtnPort`.
`VEN/src/weather.rs`'s `MqttWeatherAdapter` is the concrete implementation, subscribed via
`paho-mqtt`/`rumqttc`. Any process publishing the documented schema to the topic counts as a
"plugin" — decoupled from VEN's process lifecycle entirely (retained messages mean a
reconnecting VEN gets the last forecast immediately). The production feed for ven-1/2/3 is
the Hometown site, published by a separate `data_acquisition` project from SRF Meteo data.

## Two consumers, one resolution path

Weather reaches PV through one gate, `entities::solar::fresh_weather_pv_series` (the forecast series when fresh
and configured, nothing otherwise). It is evaluated **per PV array**: each array declares its own `weather:`
geometry and rating in the VEN profile, and `tasks/sim_tick/feeds.rs::resolve_weather_pv_for_tick` makes one
`latest()` fetch and one series evaluation per array per tick, threaded onto the array as
`TickOverrides.pv_weather_forecast`. Because the live array and the planner read the same series, a plan and the
headroom drawn against it cannot disagree.

- **Planner input** (W3): each PV plans with its own forecast (`uncurtailed_power_kw_at`), so the planner reads
  weather only through the PV asset; the sim-inject pin `pv_plan_kw` still wins. See [[milp-planner]].
- **Simulator ground truth**: the array's `weather_power_kw`, used by `step_inner` in place of the sin model —
  precedence manual sim-inject override > **measured** > weather > sin model (measured added by
  [[real-measurement-mqtt]]; a decaying manual-override offset blends additively onto whichever base wins).
- **API-visible path**: `services::forecast::build_weather_pv_forecast`, tagged `ForecastSource::WeatherModel`,
  feeds `GET /weather` and the VEN UI Weather tab (`WeatherRawPanel`/`WeatherDerivedPanel`) — the required UI
  surface for this feed per the `ui-transparency` rule (`.claude/CLAUDE.md`).

`services/forecast.rs::slot_confidence` decays confidence linearly to a 0.2 floor at 48 h age
(`base_confidence(age_h)`) — a starting default, not yet tuned against real forecast-accuracy data.

## Calibration is per-VEN and empirical

`profile/weather_pv.rs`'s `weather:` section on a `type: pv` asset (`VEN/profiles/ven-{1,2,3}.yaml`) sets
`rated_kwp` and `performance_ratio` per array. These were fit against two real observed
InfluxDB irradiance/power peaks, not left at spec defaults — ven-1's 14.4 kWp reflects a real
installation, ven-2/ven-3 are hypothetical (12 kWp / 6→8 kWp), and `performance_ratio` moved
0.87 → 0.84 during calibration. A future PV site added to this lab needs the same manual
calibration pass, not just a config value copied from an existing VEN.

## Source liveness

`MqttWeatherAdapter::is_alive()` is exposed through `WeatherForecastPort` (all implementors, including the test
mock) as `source_alive` on `GET /weather`, distinct from `is_fresh`: transport health versus content age — a
broker connection can be up with a stale retained message, or the reverse. The VEN UI Weather page shows it as
a chip (`ui-transparency`). `weather_forecast.feature` pins it with a status-topic heartbeat scenario.

## Known deferred gaps

Horizon/shading obstructions and the Perez/HDKR diffuse-sky model are deliberately not modelled
(the isotropic-on-zenith transposition stays). The snow-cover model's starting state is observed
by the PV asset from its own output against the weather (R-55, `docs/architecture/weather_forecast.md`
"Snow cover"). `docs/reference/TECHNICAL_DEBTS.md` R-54: the Mosquitto broker accepts anonymous
publishes on its plaintext listener (acceptable on the trusted lab LAN, revisit before any wider
exposure).

## E2E fixture

`weather_forecast.feature`'s `_sample_forecast_message` publishes 24 hourly samples, not one: with real
solar-position transposition against the site's coordinates a single sample could land after sunset and zero
every slot's `forecast_ac_kw`, so the "non-zero, weather-influenced PV forecast" assertion
(`tests/features/steps/weather_forecast_steps.py`) needs guaranteed daytime coverage inside the plan horizon
whatever the wall-clock time of the run.

## Relationship to the deviation arbiter

The weather forecast closes the "plan doesn't model PV" precondition [[deviation-arbiter]]'s
design once depended on — the plan's marginal-cost duals (`docs/architecture/ven_milp_planner.md`
§9) are only meaningful once PV forecast error stops dominating the deviation signal. Whether the
forecast measurably reduces PV-driven deviation/absorption events is still an open, unquantified
question.
