Feature: Outbound flexibility and forecast reports (WP3.6 — BL-10, §8.8)
  The VEN reports its flexibility envelope (IMPORT/EXPORT_RESERVATION_CAPACITY)
  and its planned consumption (USAGE_FORECAST) when an event's reportDescriptor
  requests those payload types — descriptor-driven through the same obligation
  machinery as measurement reports.

  Background:
    Given I have a VTN token as "any-business"

  Scenario: IMPORT_RESERVATION_CAPACITY descriptor yields an envelope-valued report
    Given I create a program named "envelope-report-test" and save its ID
    And I create an event for the saved program with a reportDescriptor of type "IMPORT_RESERVATION_CAPACITY" and frequency 5 seconds
    When I wait for VEN-1 to have at least 1 event
    And I wait for VEN-1 to submit an obligation-driven report for the event
    Then the latest VEN-1 report for the event has a "IMPORT_RESERVATION_CAPACITY" payload with a non-negative number value

  Scenario: USAGE_FORECAST descriptor yields a plan-slot forecast report
    Given I create a program named "usage-forecast-test" and save its ID
    And I create an event for the saved program with a reportDescriptor of type "USAGE_FORECAST" and frequency 5 seconds
    When I wait for VEN-1 to have at least 1 event
    And I wait for VEN-1 to submit an obligation-driven report for the event
    Then the latest VEN-1 report for the event has multiple intervals
    And every interval of the latest report has a "USAGE_FORECAST" payload with a number value
    And every interval of the latest report has an intervalPeriod start

  @wp5-4
  Scenario: BASELINE descriptor yields an event-blind heuristic forecast report
    Given I create a program named "baseline-report-test" and save its ID
    And I create an event for the saved program with a reportDescriptor of type "BASELINE" and frequency 5 seconds
    When I wait for VEN-1 to have at least 1 event
    And I wait for VEN-1 to submit an obligation-driven report for the event
    Then the latest VEN-1 report for the event has a "BASELINE" payload with a non-negative number value
    And every interval of the latest report has a "DATA_QUALITY" payload with value "HEURISTIC"

  @r-43
  Scenario: A successful obligation-driven report submission is recorded in GET /history/reports
    Given I create a program named "history-reports-test" and save its ID
    And I create an event for the saved program with a reportDescriptor of type "USAGE" and frequency 5 seconds
    When I wait for VEN-1 to have at least 1 event
    And I wait for VEN-1 to submit an obligation-driven report for the event
    Then VEN-1's report history includes an entry for the event
