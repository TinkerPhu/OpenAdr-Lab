# Proposal

## Why

The two exogenous forecasts of a site, PV from weather and base load from learned history, are computed
today inside each VEN, so only that VEN sees them. Other consumers (the kiosk and digest of the home, the
advisor's best-time step, a second VEN) have no way to read them except through the VEN's REST plan, which
the digest reads as a stopgap. The requirement behind this change: forecasts are produced in this repository
and published over MQTT. The Lab stays independent: the container alone is enough, no InfluxDB, no other
project. Only exogenous forecasts qualify; EV, battery and heater plans depend on the planner's own decisions
and stay inside the VEN.

## What Changes

- New container `lab-forecaster`: reads the existing weather and measurement MQTT contracts, keeps its own
  SQLite tick cache (rebuildable, training data only), and publishes retained 48 h forecasts for PV power and
  base-load power with `schema_version`, `generated_at`, model version, plus a retained status topic.
- The PV physics (`entities/solar.rs`, `pv_snow.rs`, `PvArrayGeometry`, `PvForecastParams`), the snow-state
  observation and the base-load learner (`services/heuristics.rs`) move into a new crate `lab-forecast`
  (separated first from VEN-specific types), so the VEN and the forecaster call the one implementation. The
  received forecast enters the VEN's asset as an injected parameter; the asset stays the only interpreter of
  its own forecast (asset-competence-assurance).
- Topics are per PV array plus a site total, and one for base load; every payload declares quantity, unit, sign
  and reading, carries `valid_until`, and is republished at every recompute.
- The VEN prefers the retained forecast when it is fresh and falls back to its in-process computation when the
  container is down (same shared code, so no second logic).
- The forecaster reads site geometry from the VEN profile YAML (declared once).
- A VEN UI surface shows the received forecast, its age and its source (ui-transparency).

## Capabilities

### New Capabilities
- `forecast-publication`: the retained forecast topics, their payload contract, freshness and status, and the
  VEN's preference rule between received and in-process forecast.

### Modified Capabilities
<!-- The weather and measurement feeds are unchanged on the wire; the forecaster is a second subscriber. -->

## Impact

- New crate `lab-forecast/`: PV forecast physics, snow observation, base-load learner, forecast payload
  contract. `lab-core` stays as it is.
- `VEN/src/`: `entities/solar.rs`, `pv_snow.rs`, `services/heuristics.rs` move; a forecast subscriber adapter;
  `services/forecast.rs` source preference; VEN UI forecast source display.
- New crate and service `forecaster/` with a Dockerfile and a compose entry; runs with the container alone.
- Docs: `forecasting_model.md`, `weather_forecast.md`, a new `forecast_publication.md`, a use-case file,
  a BDD scenario.
- Outside this repository: the home project's digest later reads these topics (not changed here; reported to
  the coordination session).
