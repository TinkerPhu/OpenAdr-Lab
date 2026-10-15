# Design

## Inventory: where PV and base load are single-instance today

| Concept | Today | After |
|---|---|---|
| Asset type string | `Heater/PvInverter::asset_type_str` return `ids::ASSET_*` (an id) | the type's own literal |
| Timeline per asset | `controller/timeline.rs`: `pv` → `slot.pv_used_kw`, `base_load` → `slot.baseline_kw` | `PlanTimeSlot::forecast_kw_by_asset` (each forecast participant's own power, from `results.rs`) |
| Forecast publishing | `services/forecast.rs`: PV accuracy samples and the weather PV forecast under `ASSET_PV`; base-load heuristic under `ASSET_BASE_LOAD` | per forecast asset of the roster |
| Base-load learning | `tasks/base_load_window.rs`, `simulator/recording.rs`, `tasks/sim_tick/context.rs` heuristic, `simulator/capacity_headroom.rs` events: `ASSET_BASE_LOAD` | every base-load asset under its own id |
| Residual baseline | `services/planning/service.rs`: `[ASSET_BATTERY, ASSET_EV]` | assets whose snapshot carries a SoC (`history.soc_frac`, as `residual_kwh_by_asset`) |
| Heater anchor | `services/planning/mod.rs::heater_kw` on `ASSET_HEATER` | per thermostat asset |
| EV usage view | `SimHandle::usage_schedule_view` on `ASSET_EV` | takes the asset id (route path) |
| Sim inject / config | `simulator/inject.rs` table on fixed ids; `PUT /sim/config/battery` | inject state fields name their asset; `/sim/config/{asset_id}/battery` |
| PV comms-loss ceiling | `CommsLossState::pv_ceiling_kw` reads `ASSET_PV` | sum of every array's `ac_ceiling_kw` |
| Weather geometry | top-level `weather_pv:` → `Profile::weather_pv_params` → `AppCtx`, tick context, `/weather`, forecast publishing | `PvParams::weather`; one derived series per array |
| Measured feeds | top-level `measurements:` → `pv_measurement_enabled` / `base_load_measurement_enabled` | `PvParams::measured`, `BaseLoadParams::measured`; the reading reaches only that asset |
| Tick feeds | `TickInputs::{weather_pv_kw, weather_pv_forecast, pv_measured_kw, base_load_measured_kw, base_load_heuristic*}` broadcast to every asset | per-asset maps, set per asset in the simulator loop (W5a pattern) |
| Generation limit | `pv_generation_limit_override` broadcast: each array clamps to the site limit | the site limit split over the arrays by `ac_ceiling_kw` |
| Previews | `peek_pv` / `peek_base_load` take the first PV / base load (`find_map`) | per asset; the arbiter's `measured_kw_for` reads the per-asset map |
| Base-load smoothing | one `BaseLoadSmoothingState` | one per base-load asset |
| PV irradiance smoothing | one `PvSmoothingState` | unchanged: sunlight is site-wide; each array converts it with its own geometry |

Reused: `entities::solar::fresh_weather_pv_series` (the one weather gate), `forecast_declared.rs`
(W3 participants), `SimSnapshot::assets_in_roster_order` (W5a), the W5a per-asset tick-input
pattern.

## Decisions

- **E1 (user, A).** Weather geometry and the measurement switch move into the asset's profile
  section; the old sections are refused, profiles migrated. `latitude_deg/longitude_deg` stay in
  each array's block (one struct, `WeatherPvConfig`, reused unchanged).
- **E2.** At most one PV and one base-load asset may enable a measurement: the MQTT transport has
  one topic each. Several topics are a later change.
- **E3.** Irradiance smoothing stays site-level (sunlight); base-load smoothing is per asset.
- **E4.** A site generation limit is split over the arrays in proportion to their `ac_ceiling_kw`.
- **E5.** A sim inject that targets an asset names it; with one asset of the kind the name may be
  omitted, with several it is refused (400).
- **E6.** Two merges: part 1 (per-asset loops, no profile change), part 2 (profile, feeds,
  previews, generation limit).
