Feature: Reporter multi-interval resampling (RF-05e)
  The VEN reporter produces multi-interval measurement reports when an event
  has a reportDescriptor with a specified interval duration. Each interval
  contains time-weighted mean power aggregated over the obligation period.

  Background:
    Given I have a VTN token as "any-business"

  @reporter-resampling
  Scenario: Obligation-based report contains multiple intervals
    Given I create a program named "resample-test" and save its ID
    And I create an event for the saved program with a reportDescriptor interval of "PT15M"
    When I wait for VEN-1 to have at least 1 event
    And I wait for VEN-1 to accumulate at least 20 seconds of history
    And I wait for VEN-1 to have at least 1 fulfilled obligation
    Then the latest VEN-1 report for the event has multiple intervals
    And each interval has sequential ids starting from 0
    And each interval has an intervalPeriod with start and duration "PT15M"
    And each interval contains a USAGE payload
    And each interval contains an OPERATING_STATE payload with value "ACTIVE"

  @reporter-resampling
  Scenario: Timer-driven report without reportDescriptor is single-interval
    Given I create a program named "no-descriptor-test" and save its ID
    And I create an event for the saved program without a reportDescriptor
    When I wait for VEN-1 to have at least 1 event
    And I wait for VEN-1 to submit at least 1 timer-driven report
    Then the latest VEN-1 report for the event has exactly 1 interval
