Feature: VTN Event Active Filter
  GET /events?active=true|false filters events by whether their time window
  has passed. Active events have no end time or end time in the future;
  past events have an end time strictly in the past.

  Background:
    Given I have a VTN token as "bl-client"
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

  # P-1 (docs/reference/FORK_PATCHES.md), at BDD level. Upstream has no
  # `active` param at all; without the patch the VTN pages first and filters
  # afterwards, in Rust, so `?active=true&limit=N` returns however many of that
  # page happened to be active -- a short page, or an empty one, with a 200 and
  # no indication anything was dropped. Interleaving past and active events is
  # what makes the difference observable: filter-after-page cannot fill a page
  # of 2 when every other event is past.
  Scenario: active=true fills a page even when past events are interleaved
    Given I create a past event "page-past-1" for the saved program
    And I create an open event "page-open-1" for the saved program
    And I create a past event "page-past-2" for the saved program
    And I create an open event "page-open-2" for the saved program
    And I create a past event "page-past-3" for the saved program
    And I create an open event "page-open-3" for the saved program
    When I list events for the saved program with active=true, skip 0 and limit 2
    Then the event list has exactly 2 events
    And no event in the list has ended
    When I list events for the saved program with active=true, skip 2 and limit 2
    Then the event list has exactly 1 event
    And no event in the list has ended
