Feature: Grid alert events (WP3.1, BL-04)
  ALERT_GRID_EMERGENCY / ALERT_BLACK_START events carry a human-readable
  string payload and a window (event-level intervalPeriod per User Guide
  Example 8.1-1). Both mean "minimize electricity use": the planner clamps
  the contractual import cap to 0 for slots overlapping the window, and
  releases it when the alert is deleted (deletion == cancellation in
  OpenADR 3).

  Background:
    Given I have a VTN token as "any-business"

  Scenario: ALERT_GRID_EMERGENCY clamps planned import over its window and recovers on delete
    Given I create an open program "alert-emergency-test" and save its ID
    And I create an alert event "grid-emergency-evt" of type "ALERT_GRID_EMERGENCY" for the saved program lasting 30 minutes
    When I wait for the VEN /plan to have at least one slot with import_cap_kw at most 0.1
    Then every plan slot overlapping the next 20 minutes has import_cap_kw at most 0.1
    When I delete the saved alert event
    And I wait for the VEN /plan to have no slot with import_cap_kw below 0.5

  Scenario: ALERT_BLACK_START also clamps planned import
    Given I create an open program "alert-blackstart-test" and save its ID
    And I create an alert event "black-start-evt" of type "ALERT_BLACK_START" for the saved program lasting 30 minutes
    When I wait for the VEN /plan to have at least one slot with import_cap_kw at most 0.1
    When I delete the saved alert event
    And I wait for the VEN /plan to have no slot with import_cap_kw below 0.5
