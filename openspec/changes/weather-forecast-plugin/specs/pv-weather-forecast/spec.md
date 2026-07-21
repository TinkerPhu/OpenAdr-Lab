## ADDED Requirements

### Requirement: PV array geometry parameters
The system SHALL allow configuring a PV array's geometry independently of
its electrical rating: geo-position (`latitude_deg`, `longitude_deg`),
panel tilt (`tilt_deg`, 0° = horizontal, 90° = vertical), and panel
azimuth (`azimuth_deg`, compass bearing the panel faces, 0°=N/90°=E/
180°=S/270°=W).

#### Scenario: Geometry independent of electrical rating
- **WHEN** a PV array's `PvForecastParams` is constructed
- **THEN** its geometry (location, tilt, azimuth) SHALL be configurable
  independently of `rated_kwp`, `performance_ratio`, and other electrical
  parameters

### Requirement: Clear-sky-index irradiance transposition
The system SHALL transpose a supplier's Global Horizontal Irradiance (GHI)
forecast onto the PV array's actual tilted plane using the clear-sky-index
method: computing a clear-sky model for both the horizontal plane and the
panel's own plane at the same instant, taking the forecast-to-clear-sky
ratio on the horizontal plane, and applying that ratio to the panel-plane
clear-sky value.

#### Scenario: Panel receives more irradiance than horizontal near solar noon
- **WHEN** computing plane-of-array irradiance for a south-facing,
  moderately-tilted panel at solar noon near the summer solstice at a
  mid-latitude site
- **THEN** the panel-plane irradiance SHALL be greater than the
  horizontal (GHI) irradiance for the same forecast value, because the
  sun's incidence angle on the tilted plane is smaller than on horizontal

#### Scenario: Panel facing away from the sun receives only diffuse irradiance
- **WHEN** the sun's position relative to the panel's normal vector gives
  an incidence angle greater than 90 degrees
- **THEN** the direct-beam component of the panel-plane irradiance SHALL
  be zero, and only the diffuse component SHALL contribute

#### Scenario: Nighttime irradiance is zero
- **WHEN** the sun's elevation angle at the forecast time is at or below
  the horizon
- **THEN** the computed panel-plane irradiance SHALL be zero

### Requirement: Cell-temperature derate
The system SHALL derate predicted DC power based on estimated PV cell
temperature, computed from ambient air temperature and plane-of-array
irradiance via the NOCT (Nominal Operating Cell Temperature) model, using
a configurable temperature coefficient (percent power change per degree
Celsius above 25°C).

#### Scenario: No derate at reference temperature
- **WHEN** estimated cell temperature equals 25°C (STC reference)
- **THEN** no temperature derate SHALL be applied to DC power

#### Scenario: Derate increases with cell temperature
- **WHEN** estimated cell temperature is above 25°C
- **THEN** predicted DC power SHALL be reduced proportionally to
  `temp_coeff_pct_per_c × (cell_temperature_c − 25)`

#### Scenario: Cell temperature equals air temperature at zero irradiance
- **WHEN** plane-of-array irradiance is zero (e.g. at night)
- **THEN** estimated cell temperature SHALL equal ambient air temperature

### Requirement: System performance ratio and AC clipping
The system SHALL apply a configurable system performance ratio (covering
inverter conversion loss, wiring, soiling, and mismatch losses,
independent of module conversion efficiency already reflected in the
array's rated peak power) and SHALL clamp predicted AC power to a
configurable inverter AC limit when one is set.

#### Scenario: Performance ratio scales output linearly
- **WHEN** `performance_ratio` is halved, holding all other parameters
  fixed
- **THEN** predicted AC power for that sample SHALL also be halved

#### Scenario: AC output clamped to inverter limit
- **WHEN** uncapped predicted DC-derived power for a slot exceeds the
  configured `ac_limit_kw`
- **THEN** predicted AC power for that slot SHALL be clamped to
  `ac_limit_kw`

### Requirement: Sky-condition and irradiance-variability signal
The system SHALL derive, per forecast sample, an optional
`sky_condition` (from a shared, supplier-agnostic vocabulary: Clear,
MostlyClear, PartlyCloudy, Overcast, Fog, Rain, Sleet, Snow, Thunderstorm,
Unknown) and an optional continuous `irradiance_variability` value in
[0.0, 1.0] representing how uniform or broken the sky was within that
hour, and SHALL fold `irradiance_variability` into the resulting
`AssetForecast.confidence` value.

#### Scenario: Uniform sky yields high confidence
- **WHEN** a forecast sample's `irradiance_variability` is 0.0 (uniform
  sky, whether clear or overcast)
- **THEN** the resulting PV `AssetForecast.confidence` for that slot
  SHALL NOT be reduced by the variability term

#### Scenario: Broken sky yields low confidence
- **WHEN** a forecast sample's `irradiance_variability` is 1.0 (maximally
  broken/alternating sky within the hour)
- **THEN** the resulting PV `AssetForecast.confidence` for that slot
  SHALL be reduced to reflect maximum uncertainty

#### Scenario: Missing variability signal defaults to maximum uncertainty
- **WHEN** a forecast sample has no `irradiance_variability` value
- **THEN** the system SHALL treat that slot as maximum-uncertainty rather
  than assuming stability

### Requirement: Snow-cover override
The system SHALL track a per-array snow-cover state (`covered: bool`)
across the forecast horizon: entering the covered state when forecast
new-snowfall exceeds a configurable trigger amount, and leaving the
covered state once forecast temperature reaches or exceeds a configurable
clear threshold. While covered, predicted AC power SHALL be overridden to
a configurable (default near-zero) fraction of the otherwise-computed
value, applied after all other transposition and derating steps.

#### Scenario: Fresh snowfall triggers coverage
- **WHEN** a forecast sample's new-snowfall amount meets or exceeds the
  configured trigger threshold
- **THEN** the snow-cover state SHALL become covered for that slot and
  all following slots until cleared

#### Scenario: Sustained cold keeps the panel covered
- **WHEN** the snow-cover state is covered and the forecast temperature
  for a following slot remains below the configured clear threshold
- **THEN** the snow-cover state SHALL remain covered

#### Scenario: Rising temperature clears coverage
- **WHEN** the snow-cover state is covered and a forecast sample's
  temperature reaches or exceeds the configured clear threshold
- **THEN** the snow-cover state SHALL become uncovered from that slot
  onward, absent further snowfall

#### Scenario: Coverage overrides an otherwise-high irradiance forecast
- **WHEN** the snow-cover state is covered for a slot with high forecast
  plane-of-array irradiance
- **THEN** predicted AC power for that slot SHALL be the configured
  covered-output fraction, regardless of the irradiance-based calculation

### Requirement: Weather-sourced forecast as an additive PV input
The system SHALL use the weather-sourced PV forecast (`forecast_ac_kw`) as
the planner's PV input and as the API-visible `AssetForecast` (tagged
`ForecastSource::WeatherModel`) only when a non-stale `WeatherForecast` is
available, and SHALL fall back to the existing sin-model PV forecast
otherwise, without altering sin-model behavior for any deployment that has
no weather feed configured.

#### Scenario: Weather feed available and fresh
- **WHEN** a non-stale `WeatherForecast` is available for the configured
  PV array
- **THEN** the planner's PV input and the API-visible PV forecast SHALL
  both use `forecast_ac_kw`, tagged `ForecastSource::WeatherModel`

#### Scenario: No weather feed configured
- **WHEN** no `WeatherForecastPort` adapter is configured
- **THEN** the planner's PV input and the API-visible PV forecast SHALL
  use the existing sin-model behavior, unchanged from before this change

#### Scenario: Weather feed stale
- **WHEN** the only available `WeatherForecast` has exceeded the
  configured staleness threshold
- **THEN** the planner's PV input and the API-visible PV forecast SHALL
  fall back to the existing sin-model behavior for that plan cycle
