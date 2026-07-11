Feature: Outbound flexibility and forecast reports (WP3.6 — BL-10, §8.8)
  The VEN reports its flexibility envelope (IMPORT/EXPORT_CAPACITY_RESERVATION)
  and its planned consumption (USAGE_FORECAST) when an event's reportDescriptor
  requests those payload types — descriptor-driven through the same obligation
  machinery as measurement reports.

  Background:
    Given I have a VTN token as "any-business"

  Scenario: IMPORT_CAPACITY_RESERVATION descriptor yields an envelope-valued report
    Given I create a program named "envelope-report-test" and save its ID
    And I create an event for the saved program with a reportDescriptor of type "IMPORT_CAPACITY_RESERVATION" and frequency 5 seconds
    When I wait for VEN-1 to have at least 1 event
    And I wait for VEN-1 to submit an obligation-driven report for the event
    Then the latest VEN-1 report for the event has a "IMPORT_CAPACITY_RESERVATION" payload with a non-negative number value

  Scenario: USAGE_FORECAST descriptor yields a plan-slot forecast report
    Given I create a program named "usage-forecast-test" and save its ID
    And I create an event for the saved program with a reportDescriptor of type "USAGE_FORECAST" and frequency 5 seconds
    When I wait for VEN-1 to have at least 1 event
    And I wait for VEN-1 to submit an obligation-driven report for the event
    Then the latest VEN-1 report for the event has multiple intervals
    And every interval of the latest report has a "USAGE_FORECAST" payload with a number value
    And every interval of the latest report has an intervalPeriod start
