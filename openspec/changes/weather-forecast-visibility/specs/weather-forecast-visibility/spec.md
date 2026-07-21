## ADDED Requirements

### Requirement: GET /weather returns the most recent raw forecast
The system SHALL expose `GET /weather` returning the most recently received
`WeatherForecast` (up to its full available horizon, currently up to 48
hourly samples) under a `raw` key, with no history of past forecasts —
always the single most recent one.

#### Scenario: A forecast has been received
- **WHEN** `WeatherForecastPort::latest()` returns `Some(forecast)`
- **THEN** the response's `raw` field SHALL contain that forecast's
  source_id, location, fetched_at, and full sample list

#### Scenario: No forecast has ever been received
- **WHEN** `WeatherForecastPort::latest()` returns `None`
- **THEN** the response's `raw` field SHALL be `null` and `status` SHALL be
  `"no_forecast"`, not an error response

#### Scenario: The cached forecast has gone stale
- **WHEN** a forecast is present but `WeatherForecast::is_fresh()` is false
- **THEN** `raw` SHALL still be returned (not hidden) and `status` SHALL be
  `"stale"`, so staleness is visible rather than silently indistinguishable
  from a fresh forecast

### Requirement: GET /weather returns derived PV forecast state when configured
The system SHALL compute and return, under a `derived` key, the
weather-sourced PV forecast (kW per hour) and snow-cover state (covered/not
per hour) over the same horizon as `raw`, whenever a PV-forecast
configuration section is present in the active profile — independent of
whether that configuration also feeds the planner (a separate concern,
R-50).

#### Scenario: PV forecast config is present
- **WHEN** the active profile has a `weather_pv` configuration section and
  a raw forecast is available
- **THEN** `derived` SHALL contain one entry per raw sample with its
  computed `forecast_ac_kw` and `snow_covered` flag

#### Scenario: No PV forecast config is present
- **WHEN** the active profile has no `weather_pv` configuration section
- **THEN** `derived` SHALL be `null`, regardless of whether `raw` is
  present, fresh, or stale

#### Scenario: Derived computation reuses the shared series function
- **WHEN** the derived series is computed
- **THEN** it SHALL be produced by the same function usable by a future
  planner-integration change (R-50), so the two consumers can never
  silently diverge on how the forecast is turned into a PV/snow series

### Requirement: Optional, additive PV-forecast profile configuration
The system SHALL support an optional profile YAML section describing PV
array geometry and forecast parameters (location, tilt, azimuth, rated
peak power, performance ratio, temperature coefficient, NOCT, AC limit,
snow-cover parameters), absent by default, so every existing profile
continues to parse and behave identically without it.

#### Scenario: Profile without the new section
- **WHEN** a profile YAML has no PV-forecast configuration section
- **THEN** the profile SHALL parse successfully exactly as before this
  change, with no validation error and no behavior change to any existing
  capability

#### Scenario: Profile with the new section
- **WHEN** a profile YAML includes the PV-forecast configuration section
- **THEN** it SHALL be parsed into the same `PvForecastParams`/
  `PvArrayGeometry`/`PvSnowParams` types already defined in
  `entities::asset_params`, with no duplicate type definitions

### Requirement: VEN UI shows weather on its own tab
The system SHALL provide a VEN UI tab named "Weather" displaying the raw
forecast (temperature, irradiance, sky condition per hour) and, when
available, the derived PV forecast (kW, snow-covered per hour) over the
48-hour horizon, and SHALL rename the existing "Planner" tab to "Plan"
without changing or removing any of its existing content.

#### Scenario: Weather tab with both raw and derived data
- **WHEN** `GET /weather` returns both `raw` and `derived`
- **THEN** the Weather tab SHALL display both, covering the full returned
  horizon

#### Scenario: Weather tab with no forecast configured
- **WHEN** `GET /weather` returns `status: "no_forecast"`
- **THEN** the Weather tab SHALL show a clear "no weather feed configured"
  state, not an error or a blank page

#### Scenario: Plan tab content is unchanged
- **WHEN** a user navigates to the renamed "Plan" tab
- **THEN** every element previously shown on the "Planner" tab (plan
  header, trigger timeline, decision matrix, power stack, session
  progress, trace table) SHALL still be present and unchanged
