@resilience
Feature: Failure Recovery
  The system must recover from VTN restarts, VEN restarts, and transient
  outages without manual intervention.

  Background:
    Given I have a VTN token as "any-business"

  Scenario: VEN retains cached events when VTN goes down
    Given I create an open program "resilience-cache" and save its ID
    And I create an event for the saved program named "cache-evt"
    And I wait for VEN-1 to show event "cache-evt"
    When the "test-vtn" service is stopped
    Then VEN-1 still serves cached event "cache-evt"

  Scenario: VEN re-syncs after VTN restart
    Given I create an open program "resilience-resync" and save its ID
    And I create an event for the saved program named "resync-evt-1"
    And I wait for VEN-1 to show event "resync-evt-1"
    When the "test-vtn" service is restarted
    And I wait for the "test-vtn" service to be healthy
    And I refresh my VTN token as "any-business"
    And I create an event for the saved program named "resync-evt-2"
    Then VEN-1 picks up event "resync-evt-2" within 30 seconds

  Scenario: Both VENs converge after VTN restart
    Given I create an open program "resilience-dual" and save its ID
    And I create an event for the saved program named "dual-evt"
    And I wait for VEN-1 to show event "dual-evt"
    And I wait for VEN-2 to show event "dual-evt"
    When the "test-vtn" service is restarted
    And I wait for the "test-vtn" service to be healthy
    And I refresh my VTN token as "any-business"
    And I create an event for the saved program named "dual-evt-2"
    Then VEN-1 picks up event "dual-evt-2" within 30 seconds
    And VEN-2 picks up event "dual-evt-2" within 30 seconds

  Scenario: VEN recovers after its own restart
    Given I create an open program "resilience-ven-restart" and save its ID
    And I create an event for the saved program named "ven-restart-evt"
    And I wait for VEN-1 to show event "ven-restart-evt"
    When the "test-ven-1" service is restarted
    And I wait for the "test-ven-1" service to be healthy
    Then VEN-1 picks up event "ven-restart-evt" within 30 seconds

  # WP2.1 (BL-03): poll loops back off exponentially with jitter while the VTN
  # is down, instead of hammering it every poll_events_secs (30s in this
  # stack). 130s of outage covers two backoff steps (~30s, then ~60s), long
  # enough to observe growing intervals despite ±10% jitter.
  #
  # Recovery-pickup timeout is deliberately wide (180s, not the usual 30s):
  # when the outage ends, the poll loop may already be mid-sleep in a
  # previously-computed backoff delay (up to ~120-130s here, since a third
  # failure/backoff step can fire right before the outage ends) — the reset
  # to the base interval only takes effect on the *next* successful poll, not
  # instantly on VTN recovery. That bounded-but-longer recovery latency is the
  # deliberate backoff trade-off (never hammering a still-recovering VTN), not
  # a regression — confirmed via a live Pi4 run where the actual wait was
  # ~130-160s.
  Scenario: VEN backs off exponentially during a sustained VTN outage
    Given I create an open program "resilience-backoff-recovery" and save its ID
    And I mark the current time as the outage start
    When the "test-vtn" service is stopped
    And I wait 130 seconds
    Then VEN-1's events-poll failure log shows growing intervals since the outage start
    When the "test-vtn" service is restarted
    And I wait for the "test-vtn" service to be healthy
    And I refresh my VTN token as "any-business"
    And I create an event for the saved program named "backoff-recovery-evt"
    Then VEN-1 picks up event "backoff-recovery-evt" within 180 seconds
