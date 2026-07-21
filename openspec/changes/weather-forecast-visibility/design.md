## Context

`WeatherForecastPort`/`MqttWeatherAdapter` (weather-forecast-plugin) cache
the latest received `WeatherForecast` in `AppCtx.weather`, and
`entities::solar`/`entities::pv_snow` have the pure physics to derive a PV
forecast + snow-cover state from it — but nothing reads any of it. This
design covers the read-only route + UI surface that closes that gap,
without touching the deferred planner wiring (R-50).

Existing patterns this follows: route files under `VEN/src/routes/`, one
file per concern, registered in `routes/mod.rs`'s router
(`GET /forecast/:asset_id`, `GET /capability/:asset_id` etc. show the
existing per-resource GET shape); VEN UI pages under `ui/src/pages/` with
supporting components under `ui/src/components/<area>/` (the Planner page's
own `components/planner/` directory is the direct model); the existing
`ui-transparency` rule and its WP-T1..T8 precedent (Dashboard status rows,
Diagnostics menu group).

## Goals / Non-Goals

**Goals:**
- Make the most recently received weather forecast, and its derived PV/
  snow state over the same 48-hour horizon, visible via one GET endpoint
  and one VEN UI tab.
- Do this without depending on R-50 (planner wiring) — a config surface
  for `PvForecastParams` is enough to compute the derived series
  standalone.
- Keep the Planner tab's existing content fully intact, just renamed and
  no longer sharing space with weather.

**Non-Goals:**
- Feeding the derived series into the actual MILP planner input (R-50).
- Any forecast history/trend (only ever the single latest forecast).
- A profile UI/editor for the new config section — YAML only, like every
  other asset param today.

## Decisions

- **One route, `GET /weather`, not two.** Raw and derived state are always
  about the same forecast and the same horizon; splitting them into
  `/weather/raw` + `/weather/derived` would just force the UI to make two
  requests and reconcile timestamps itself. A single response with two
  top-level keys (`raw`, `derived`) mirrors how `AssetForecast` already
  bundles source + values in one object.
- **`derived` is `null` when no `PvForecastParams` config exists**, not an
  error. Matches the existing pattern of `WeatherForecastPort` itself:
  absence of configuration degrades to "nothing to show," never a 4xx —
  a VEN with a weather feed but no PV geometry configured yet is a normal,
  supported state (e.g. mid-rollout), not a client error.
- **The derived-series computation lives next to the physics it calls**
  (`entities::solar`), as a new pure function
  `weather_pv_forecast_series(params, forecast) -> Vec<WeatherPvForecastSlot>`
  — not inside the route handler — so it's unit-testable the same way
  `forecast_ac_kw` already is, and so a future R-50 implementation can call
  the *same* function for the planner's own input instead of duplicating
  the loop-and-zip-with-snow-trajectory logic.
- **Snow-cover initial state uses the forecast-only fallback**
  (`PvSnowState::default()`, folded from the forecast's own `age_h=0`
  sample) — the same convention already established and tested in
  `entities::pv_snow`. The telemetry cross-check (R-55) is still out of
  scope.
- **Config section is optional and additive** (`weather_pv: Option<...>`
  in the profile schema) so every existing profile YAML keeps parsing and
  behaving identically with no changes. Mirrors how `history.enabled`
  gates the optional history store today.
- **UI: rename, don't rebuild.** "Planner" → "Plan" (nav label + route
  stays `/planner` — no deep-link breakage) with identical content; new
  "Weather" tab at `/weather` is a new page, not a retrofit of the
  Planner page. Keeps the change's blast radius to "one new page, one
  renamed label" rather than touching `PlanHeaderBar`/`PlanPowerStack`/etc.

## API shape

```
GET /weather
```

```json
{
  "status": "ok",
  "is_fresh": true,
  "raw": {
    "source_id": "srf_meteo",
    "location": { "latitude_deg": 47.4491, "longitude_deg": 7.8081 },
    "fetched_at": "2026-07-19T05:54:48Z",
    "samples": [
      { "valid_at": "2026-07-19T06:00:00Z", "age_h": 1, "temperature_c": 16.0,
        "ghi_w_m2": 97.0, "sky_condition": "partly_cloudy", "irradiance_variability": 0.6 }
    ]
  },
  "derived": {
    "params": { "rated_kwp": 10.0, "tilt_deg": 30.0, "azimuth_deg": 180.0 },
    "slots": [
      { "valid_at": "2026-07-19T06:00:00Z", "forecast_ac_kw": 0.42, "snow_covered": false }
    ]
  }
}
```

- `raw: null` when `WeatherForecastPort::latest()` returns `None` (no feed
  configured or nothing received yet) — `status` becomes `"no_forecast"`.
- `status: "stale"` (raw still present) when `WeatherForecast::is_fresh()`
  is false — shown, not hidden, so the UI can flag it rather than silently
  displaying old numbers as current.
- `derived: null` whenever the profile has no `weather_pv` section,
  regardless of `raw`'s presence/freshness.

## Risks / Trade-offs

- **[Risk]** A route exposing derived PV physics duplicates knowledge of
  "how PV forecast is computed" outside the planner, and R-50 will need to
  reuse the exact same function to avoid the two ever silently diverging.
  → **Mitigation**: the decision above (shared `weather_pv_forecast_series`
  function) exists specifically so R-50 calls the same code path rather
  than re-deriving it.
- **[Risk]** Renaming a nav label/tab touches existing UI tests
  (`App.test.tsx` nav-visibility assertions, per the WP-T8 journal entry
  about exactly this kind of change). → **Mitigation**: budgeted as its own
  task; expected, not a surprise, given the WP-T8 precedent already
  documented this exact test-update pattern.
- **[Trade-off]** Showing `derived: null` when unconfigured means most
  VEN deployments will see an empty derived section until someone fills in
  `weather_pv` in their profile YAML. Acceptable — matches the "additive,
  no behavior change for unconfigured deployments" property already
  established for the whole weather-forecast-plugin feature.

## Open Questions

- Exact profile YAML key name/shape for `weather_pv` — proposed above,
  finalize during implementation against `profile/schema.rs`'s existing
  style for asset params.
- Whether `GET /weather`'s `derived.params` should echo back the full
  config or just enough for the UI to label the chart (kept minimal in
  the shape above; revisit if the UI needs more).
