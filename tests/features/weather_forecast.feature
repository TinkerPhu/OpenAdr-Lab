Feature: Weather Forecast Plugin — MQTT-sourced PV forecast
  A weather forecast published over MQTT (docs/plans/weather-forecast-plugin.md)
  replaces the sin-model PV forecast in both the planner's own input and the
  API-visible /forecast/pv response, falling back to the sin model when no
  forecast is available or the cached one has gone stale.

  Background:
    Given the VEN is running with profile "test"

  @weather-forecast
  Scenario: Fresh weather forecast changes the planner's PV allocation
    Given a weather forecast message is published to the test Mosquitto broker for VEN-1
    When a plan cycle runs on VEN-1
    Then the plan's PV allocation reflects the weather-sourced forecast rather than the sin model

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
