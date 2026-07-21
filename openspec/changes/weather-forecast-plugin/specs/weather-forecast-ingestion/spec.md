## ADDED Requirements

### Requirement: WeatherForecastPort seam
The system SHALL expose a `WeatherForecastPort` trait with an
`async fn latest(&self) -> Option<WeatherForecast>` method as the sole
in-process boundary between weather-consuming code (services, planner)
and any weather data source. Consuming code SHALL NOT depend on MQTT, JSON
parsing, or any supplier-specific detail directly.

#### Scenario: No weather source configured
- **WHEN** VEN starts with no weather MQTT broker configured
- **THEN** `WeatherForecastPort::latest()` SHALL return `None`
- **AND** all weather-dependent behavior SHALL fall back to its
  pre-existing non-weather behavior (e.g. the PV sin-model forecast)

#### Scenario: Forecast available
- **WHEN** a valid forecast message has been received by the adapter
- **THEN** `WeatherForecastPort::latest()` SHALL return that forecast
  without performing any network I/O on the calling path

### Requirement: MQTT transport and topic contract
The system SHALL implement an MQTT-based adapter for `WeatherForecastPort`
that subscribes to two topics under `<root>/weather/<site_id>/`:
`forecast` (the data topic) and `status` (the heartbeat topic). Both
topics SHALL use QoS 1 and the retained flag, and `<root>` and `site_id`
SHALL be configurable per deployment.

#### Scenario: Late subscriber receives last known state
- **WHEN** a VEN instance (re)connects to the broker after the plugin has
  already published at least one message on a topic
- **THEN** VEN SHALL receive that topic's last retained message
  immediately, without waiting for the next publish

#### Scenario: Multiple sites on one broker
- **WHEN** two different `site_id` values are configured for two VEN
  deployments sharing one broker
- **THEN** each VEN instance SHALL only receive messages for its own
  configured `site_id`, requiring no code change to add a new site

### Requirement: Forecast message validation
The system SHALL validate every incoming `forecast` topic message against
the documented schema (required fields: `schema_version`, `source_id`,
`location`, `fetched_at`, `samples`; each sample requiring `valid_at`,
`age_h`, `temperature_c`, `ghi_w_m2`) before accepting it. A message
failing validation SHALL be rejected and logged, and SHALL NOT crash the
adapter process or corrupt the previously cached forecast.

#### Scenario: Missing required field
- **WHEN** an inbound `forecast` message omits a required field (e.g.
  `fetched_at`)
- **THEN** the adapter SHALL reject the message, log the rejection, and
  leave the previously cached forecast (if any) unchanged

#### Scenario: Out-of-range value
- **WHEN** an inbound sample's `temperature_c` or `ghi_w_m2` falls outside
  its documented bound
- **THEN** the adapter SHALL reject the entire message rather than
  accepting partially valid data

#### Scenario: Forward-compatible unknown fields
- **WHEN** an inbound message contains JSON object keys not defined in the
  documented schema
- **THEN** the adapter SHALL ignore the unknown keys and process the
  message normally, provided all required fields are present and valid

### Requirement: Status heartbeat and liveness detection
The system SHALL track the most recently received `status` topic message
per configured source, and SHALL treat a source as dead if no `status`
message (retained or live) has been observed within twice the documented
heartbeat interval.

#### Scenario: Plugin crashes ungracefully
- **WHEN** a plugin process disconnects from the broker without a clean
  shutdown
- **THEN** the broker-published Last Will and Testament message with
  `status: "offline"` SHALL be treated by VEN as an immediate liveness
  signal, without waiting for the heartbeat timeout window

#### Scenario: Plugin stops publishing without disconnecting
- **WHEN** no `status` message has been received for longer than twice the
  documented heartbeat interval
- **THEN** VEN SHALL treat the source as dead even though no disconnect
  event occurred

### Requirement: Weather forecast staleness policy
The system SHALL reject a cached `WeatherForecast` for planning purposes
once the elapsed time since its `fetched_at` exceeds a configurable
staleness threshold, falling back to non-weather-sourced behavior for any
consumer that would otherwise have used it.

#### Scenario: Forecast within staleness threshold
- **WHEN** a cached forecast's `fetched_at` is within the configured
  staleness threshold of the current time
- **THEN** consumers SHALL treat it as trustworthy and use it

#### Scenario: Forecast past staleness threshold
- **WHEN** a cached forecast's `fetched_at` is older than the configured
  staleness threshold
- **THEN** consumers SHALL NOT use it for planning purposes and SHALL fall
  back to their pre-existing non-weather behavior, even though
  `WeatherForecastPort::latest()` still technically returns the stale
  value
