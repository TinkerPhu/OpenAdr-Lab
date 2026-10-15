# Proposal

## Why

W1–W5a let the planner and real-time control handle several assets of a kind, but PV and base
load are single-instance by construction outside the planner: one site-level `weather_pv:`
geometry, one `measurements:` switch per kind, one base-load smoothing state, live previews that
take the first PV / base-load asset, and per-id lookups in the timeline, forecast publishing, the
base-load learning window and recorder. Two PV arrays (east and west roof) would both get the same
weather estimate and the same measured kW. This is W5b, the second half of W5 (user decision
D5 A, 2026-10-15: per asset, breaking profile change).

## What Changes

- **BREAKING (profile):** a PV asset carries its own `weather:` block (today's `weather_pv:`
  fields) and `measurement: {enabled}`; a base-load asset carries `measurement: {enabled}`. The
  top-level `weather_pv:` and `measurements:` sections are refused with a message naming the new
  place. Every profile in the repo is migrated in the same change.
- Each PV array's weather forecast is computed from its own geometry and kWp, so east and west
  arrays get different power curves; the planner sees each (W3 participants) and the plan keeps
  each forecast asset's own curve (`PlanTimeSlot::forecast_kw_by_asset`).
- Measured feeds, live previews, base-load smoothing, the learned heuristic, the trailing load
  window and the recorder run per asset; the arbiter's measured-feed table becomes per asset.
- `GET /weather` returns one derived forecast per PV asset; the VEN UI shows each.
- Remaining fixed-id lookups become per-asset loops: timeline, forecast publishing, comms-loss PV
  ceiling (sum of the arrays), residual baseline, heater anchor, EV usage view, sim inject
  targets and `/sim/config/{asset_id}`.

## Capabilities

### New Capabilities
- `pv-base-load-per-asset`: each PV array and base load has its own forecast, measurement and
  history, whatever its id.

## Impact

- VEN profile schema and validation; `boot/`, `tasks/sim_tick/*`, `simulator/*`, `assets/pv*`,
  `assets/base_load*`, `services/forecast.rs`, `controller/timeline.rs`, `routes/weather.rs`,
  `routes/sim.rs`, VEN UI weather page and sim controls.
- One-array profiles (ven-1 and all others): identical behaviour once migrated.
- **Out of scope:** one meter shared by several arrays (filed as a backlog item); per-array snow
  state (site-level, as today); several measurement topics (validation allows one measured PV and
  one measured base load, as many as the MQTT transport has topics).
