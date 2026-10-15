Feature: Weather Forecast Plugin — MQTT-sourced PV forecast
  A weather forecast published over MQTT (docs/architecture/weather_forecast.md)
  replaces the sin-model PV forecast in the planner's input, falling back to
  the sin model when no
  forecast is available or the cached one has gone stale.

  Background:
    Given the VEN is running with profile "test"

  @weather-forecast
  Scenario: Fresh weather forecast changes the planner's PV allocation
    Given a weather forecast message is published to the test Mosquitto broker for VEN-1
    When a plan cycle runs on VEN-1
    Then the plan's PV allocation reflects the weather-sourced forecast rather than the sin model

  # W5b: each PV array carries its own geometry (its profile `weather:` block), so the
  # derived forecast is per array, named by the array's id.
  @weather-forecast
  Scenario: Each PV array gets its own derived forecast
    Given a weather forecast message is published to the test Mosquitto broker for VEN-1
    Then /weather derives a PV forecast for the array "pv"

  @weather-forecast
  Scenario: No weather forecast configured falls back to the sin-model PV forecast
    Given no weather forecast has ever been published for VEN-1
    When a plan cycle runs on VEN-1
    Then the plan's PV allocation matches the sin-model forecast

  @weather-forecast
  Scenario: Stale weather forecast falls back to the sin-model PV forecast
    Given a weather forecast message older than the staleness threshold is published to the test Mosquitto broker for VEN-1
    When a plan cycle runs on VEN-1
    Then the plan's PV allocation matches the sin-model forecast

  @weather-forecast
  Scenario: Weather status heartbeat marks the source alive independent of forecast content
    Given a weather forecast message is published to the test Mosquitto broker for VEN-1
    Then /weather reports the source as not alive
    When a weather status heartbeat is published to the test Mosquitto broker for VEN-1
    Then /weather reports the source as alive
