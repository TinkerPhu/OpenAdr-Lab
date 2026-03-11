Feature: VTN Event Active Filter
  GET /events?active=true|false filters events by whether their time window
  has passed. Active events have no end time or end time in the future;
  past events have an end time strictly in the past.

  Background:
    Given I have a VTN token as "any-business"
    And I create a program named "active-filter-program" and save its ID

  Scenario: active=true returns only current events
    Given I create a past event "past-evt-1" for the saved program
    And I create an open event "open-evt-1" for the saved program
    When I list events for the saved program with active=true
    Then the event list contains "open-evt-1"
    And the event list does not contain "past-evt-1"

  Scenario: active=false returns only past events
    Given I create a past event "past-evt-2" for the saved program
    And I create an open event "open-evt-2" for the saved program
    When I list events for the saved program with active=false
    Then the event list contains "past-evt-2"
    And the event list does not contain "open-evt-2"

  Scenario: no active filter returns all events
    Given I create a past event "past-evt-3" for the saved program
    And I create an open event "open-evt-3" for the saved program
    When I list events for the saved program
    Then the event list contains "past-evt-3"
    And the event list contains "open-evt-3"
