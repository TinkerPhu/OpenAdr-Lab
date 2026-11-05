Feature: Forecast publication — the lab-forecaster publishes retained PV forecasts
  The lab-forecaster container turns the weather forecast into retained MQTT messages
  (docs/architecture/forecast_publication.md): one PV series per array, their site total, and a status,
  each declaring what its numbers mean. A reader needs only the broker.

  @forecast-publication
  Scenario: A weather forecast becomes a retained, declared PV forecast
    Given a weather forecast message is published to the test Mosquitto broker for VEN-1
    Then the lab-forecaster has published a retained PV forecast for the array "pv" of VEN-1's site
    And that forecast declares power in kW, positive for generation, natural, as the average over each step
    And that forecast is valid for 90 minutes and covers 24 hours
    And the retained site total equals the PV forecast of the array "pv"

  @forecast-publication
  Scenario: The status says the producer is healthy
    Given a weather forecast message is published to the test Mosquitto broker for VEN-1
    Then the lab-forecaster has published a retained PV forecast for the array "pv" of VEN-1's site
    And the lab-forecaster's status is "ok" and names its producer and the age of its inputs

  @forecast-publication
  Scenario: A stale weather forecast is reported and the forecast is not refreshed
    Given a weather forecast message is published to the test Mosquitto broker for VEN-1
    Then the lab-forecaster has published a retained PV forecast for the array "pv" of VEN-1's site
    Given a weather forecast message older than the staleness threshold is published to the test Mosquitto broker for VEN-1
    Then the lab-forecaster's status becomes "weather_stale"
    And the retained PV forecast of the array "pv" is not refreshed while the weather stays stale
