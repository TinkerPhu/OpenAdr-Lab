## Why

The weather-forecast-plugin work (`openspec/changes/weather-forecast-plugin/`)
added a `WeatherForecastPort`, an MQTT adapter, and the PV transposition/
snow-cover physics — but nothing reads `AppCtx.weather` anywhere: no route,
no UI panel. That's a real gap, not a deferred nicety: it directly violates
the newly-adopted `ui-transparency` rule (`.claude/CLAUDE.md`) — every
backend capability must have a visible surface — and is tracked as R-57 in
`docs/reference/TECHNICAL_DEBTS.md`. This change closes it: a `GET /weather`
route exposing both the raw received forecast and its derived state, and a
VEN UI split of the crowded Planner tab so weather gets its own visible home.

Scoped deliberately **not** to require R-50 (the still-deferred planner/
`SolveRequest` wiring): the derived state here is a read-only diagnostic
computation over the cached forecast, independent of what the actual MILP
planner uses for its own PV input. R-50 remains separate follow-up work.

## What Changes

- Add a minimal PV-forecast-params config surface (profile YAML, closes
  R-51 for this purpose only — not the full planner-integration config):
  geo-position, tilt/azimuth, rated_kwp, performance_ratio, temp
  coefficient, NOCT, AC limit, snow params. Optional section; absent by
  default, so every existing profile keeps working unchanged.
- Add a pure function computing the full weather-sourced PV forecast
  series (kW + snow-covered flag per hour) over a `WeatherForecast`'s
  samples, reusing `entities::solar::forecast_ac_kw` and
  `entities::pv_snow::snow_coverage_trajectory` — no planner involvement.
- Add `GET /weather`: returns the most recent raw `WeatherForecast` (up to
  48 hourly samples) plus, when the config section above is present, the
  derived series for the same horizon. No history — always the single most
  recent forecast, per the "always show the most recent" requirement.
- VEN UI: split the Planner tab into a shortened **"Plan"** tab (existing
  plan/trigger/decision-matrix/power-stack/session/trace content, renamed
  only) and a new **"Weather"** tab showing the raw forecast (temperature,
  irradiance, sky condition, per-hour) and the derived PV forecast (kW,
  snow-covered) over the 48-hour horizon.
- No **BREAKING** changes — additive route, additive optional config
  section, a UI tab rename + split with no content removed.

## Capabilities

### New Capabilities
- `weather-forecast-visibility`: the `GET /weather` route (raw + derived
  state), the pure derived-series computation, the minimal PV-forecast
  config surface, and the VEN UI Weather tab.

### Modified Capabilities
(none — this doesn't change any existing requirement's behavior; the
Planner-tab rename/split is a UI reorganization with identical underlying
functionality, not a requirements change to any existing capability.)

## Impact

- **Affected service**: VEN (Rust route + profile schema) and VEN UI
  (React). No VTN, BFF, or VTN UI changes.
- **Affected files**: `VEN/src/routes/` (new route file), `VEN/src/profile/`
  (schema addition), `VEN/src/entities/solar.rs` (new series function),
  `VEN/ui/src/App.tsx` (nav split), `VEN/ui/src/pages/` (rename + new
  Weather page), `VEN/ui/src/components/weather/` (new components,
  mirroring the existing `components/planner/` pattern).
- **No new dependency** — reuses `WeatherForecastPort`/`entities::solar`/
  `entities::pv_snow` already built.
- **Debt closed**: R-57 (fully), R-51 (partially — the config surface
  exists and is consumed by this route; whether the *planner itself* also
  consumes it is R-50, untouched by this change).

## Non-goals

- Wiring the weather forecast into the actual MILP planner (`SolveRequest`)
  — that remains R-50, separate follow-up work.
- Forecast history/trend views (past forecasts, accuracy tracking) — only
  the single most recent forecast is shown, per the stated requirement.
- Multi-site display — one VEN, one configured site, matching the rest of
  this project's single-VEN-per-instance model.
