## ADDED Requirements

### Requirement: The forecaster publishes retained exogenous forecasts
The `lab-forecaster` container SHALL publish, per site, retained messages on
`<root>/forecast/<site>/pv/<array_id>/power_kw` for each PV array in the profile,
`<root>/forecast/<site>/pv/total/power_kw` (the sum of the arrays) and
`<root>/forecast/<site>/base_load/power_kw`. Each payload SHALL carry `schema_version`, `generated_at`,
`valid_until`, `model_version`, `producer_id`, `start`, `step_s`, `values`, `confidence`, `coverage_h`, and the
contract declared on the wire: `quantity`, `unit`, `sign` and `reading`. A PV payload SHALL declare that it is
natural (uncurtailed) generation with positive sign for generation; a base-load payload SHALL declare positive
for consumption.

#### Scenario: Weather forecast arrives
- **WHEN** a valid weather forecast arrives on the weather topic of a configured site
- **THEN** every PV topic of that site is republished retained with the `generated_at` of the recompute

#### Scenario: A new client subscribes
- **WHEN** a new MQTT client subscribes to the forecast topics
- **THEN** it receives all retained forecasts without waiting for the next recompute

#### Scenario: Declared contract
- **WHEN** any forecast payload is published
- **THEN** it declares quantity, unit, sign and reading, and the payload validator refuses to build one without them

#### Scenario: Site total
- **WHEN** a site has two PV arrays
- **THEN** `pv/total/power_kw` equals the sum of the two array series slot by slot

### Requirement: The published coverage is what exists
`coverage_h` SHALL state the horizon actually delivered, at most what the weather feed provides for PV and 48 h
for base load, and the forecaster SHALL NOT pad a series beyond its source.

#### Scenario: Short weather horizon
- **WHEN** the weather feed covers 30 hours
- **THEN** the PV series has `coverage_h` of 30 and no values beyond that

#### Scenario: Full horizon
- **WHEN** the weather feed covers at least 48 hours
- **THEN** the PV series and the base-load series both cover 48 hours

### Requirement: Freshness is stated by the producer
The forecaster SHALL republish at every recompute, also when the series is unchanged, and each payload SHALL
carry `valid_until`; consumers SHALL treat a forecast past `valid_until` as stale. The freshness limit of a series is
`valid_until - generated_at` (default 90 minutes for every series, set in the forecaster's settings and stated in
the payload), so a consumer's catalogue can carry the same number per series.

#### Scenario: Unchanged series
- **WHEN** a recompute yields the same series as the last
- **THEN** the topic is still republished with a newer `generated_at` and `valid_until`

### Requirement: The forecaster states its health
The container SHALL publish a retained `<root>/forecast/<site>/status` with its state (`ok`, `cold_start`,
`weather_stale`, `degraded`), the age of each input and its `producer_id`, and a last will setting it to
`offline`.

#### Scenario: Weather feed stale
- **WHEN** no weather forecast arrived within the freshness limit
- **THEN** status is `weather_stale` and the PV forecasts are not republished with a newer `generated_at`

#### Scenario: No history yet
- **WHEN** the tick cache holds fewer samples than the learner needs
- **THEN** the base-load forecast is published from the cold-start profile, `model_version` names it, and status is `cold_start`

### Requirement: One producer per site
The container SHALL refuse to start while a live retained status of another producer exists for the same site.

#### Scenario: Second forecaster
- **WHEN** a forecaster starts and the retained status of another producer is fresh
- **THEN** it logs the conflict and exits with a non-zero code without publishing

### Requirement: The Lab runs with the container alone
The forecaster SHALL need only the MQTT broker and its own SQLite cache; it SHALL NOT require InfluxDB or any
other project, and rebuilding the cache SHALL NOT need a backup.

#### Scenario: Cache deleted
- **WHEN** the cache file is removed and the container restarted
- **THEN** it starts, publishes status `cold_start` and recovers as ticks accumulate

### Requirement: Snow state is observed from the PV measurement
The forecaster SHALL apply the same snow-cover observation as the PV asset, to the PV measurement of the array,
so that its series equals the VEN's own computation for the same weather and measurement. With no PV measurement
configured it SHALL use the default (not covered) state and say so in the status.

#### Scenario: Panel covered
- **WHEN** the weather says snow is possible and the measured output is far below the snow-free output
- **THEN** the published PV series for that array reflects coverage, as the VEN's own computation does

#### Scenario: No measurement
- **WHEN** no PV measurement is configured for the array
- **THEN** the series uses the default snow state and the status lists `snow_state: unobserved`

### Requirement: One implementation of each forecast
The PV forecast, the snow observation and the base-load forecast SHALL each be computed by one shared function,
called by the forecaster and by the VEN fallback.

#### Scenario: Same inputs, same series
- **WHEN** the shared function is called with the same weather, geometry, measurement and clock from the forecaster and from the VEN
- **THEN** both series are identical

### Requirement: The VEN prefers a fresh received forecast
The VEN SHALL use the retained forecast of its own array and of the base load while it is valid and before
`valid_until`, SHALL convert it to its internal convention only at its boundary, SHALL hand it to the asset as an
injected parameter, and SHALL fall back to its in-process computation otherwise. Its UI SHALL show which source
is in use, the age of the received forecast and, when a received forecast was rejected, the reason.

#### Scenario: Container down
- **WHEN** the received forecast is past `valid_until`
- **THEN** the VEN plans on its own computation and the UI shows source `in-process`

#### Scenario: Container up
- **WHEN** a valid received forecast is before `valid_until`
- **THEN** the VEN plans on it and the UI shows source `forecaster` with its age

#### Scenario: Unknown major schema version
- **WHEN** a forecast arrives with an unsupported major `schema_version`
- **THEN** the VEN keeps its fallback and the UI shows the rejection reason

#### Scenario: Undeclared contract
- **WHEN** a forecast arrives without a unit, sign or quantity
- **THEN** it is rejected with that reason and the VEN keeps its fallback

#### Scenario: Non-numeric value
- **WHEN** a forecast payload holds a non-numeric value
- **THEN** it is rejected through the shared payload reader and the VEN keeps its fallback

### Requirement: Received forecasts are scored
The VEN SHALL record the received forecast against the measured outcome in `forecast_accuracy_samples`, with the
forecast's `model_version` named.

#### Scenario: Outcome recorded
- **WHEN** the hour a received forecast covered has passed and a measurement exists
- **THEN** a sample with forecast, outcome and `model_version` is stored
