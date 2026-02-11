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
    And I create an event for the saved program named "resync-evt-2"
    Then VEN-1 picks up event "resync-evt-2" within 30 seconds

  Scenario: Both VENs converge after VTN restart
    Given I create an open program "resilience-dual" and save its ID
    And I create an event for the saved program named "dual-evt"
    And I wait for VEN-1 to show event "dual-evt"
    And I wait for VEN-2 to show event "dual-evt"
    When the "test-vtn" service is restarted
    And I wait for the "test-vtn" service to be healthy
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
