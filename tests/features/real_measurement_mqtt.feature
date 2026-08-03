Feature: Real Measurement MQTT Feeds — PV and baseline load
  A real measurement published over MQTT (real-measurement-mqtt) is live
  ground truth for the current tick only — it outranks the weather/sin-model
  PV estimate (for PV) or the synthetic profile+noise baseline (for baseline
  load), and is visible via GET /measurement, falling back to the
  pre-existing simulated behavior when no reading has ever been received or
  the cached one has gone stale.

  Background:
    Given the VEN is running with profile "test"
    And the VEN-1 sim overrides are reset

  # Must run first in this feature: MQTT retains the last message on a topic,
  # so once any later scenario below publishes to the PV measurement topic,
  # "never published" no longer holds for the rest of this broker session.
  @real-measurement-mqtt
  Scenario: No measurement configured falls back to the pre-existing simulated behavior
    Given no PV measurement has ever been published for VEN-1
    Then /measurement reports the PV signal as not_configured

  @real-measurement-mqtt
  Scenario: A measured PV reading outranks the weather-derived estimate on the live tick
    # An earlier scenario elsewhere in the suite may have left a manual
    # pv_irradiance offset slowly decaying (by design — see
    # docs/history/project_journal.md's PV weather-blend fix); flush it to
    # exactly zero first so it can't additively bleed into this check.
    Given the VEN-1 pv irradiance offset is flushed to zero
    And a weather forecast message is published to the test Mosquitto broker for VEN-1
    And a PV measurement message is published to the test Mosquitto broker for VEN-1
    Then /measurement reports the PV signal as ok with the published reading
    And the live PV power on VEN-1 reflects the measured reading rather than the weather estimate

  @real-measurement-mqtt
  Scenario: A measured baseline-load reading replaces the synthetic profile
    Given a baseline-load measurement message is published to the test Mosquitto broker for VEN-1
    Then /measurement reports the base_load signal as ok with the published reading
