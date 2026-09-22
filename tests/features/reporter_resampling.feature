Feature: Reporter multi-interval resampling (RF-05e)
  The VEN reporter produces multi-interval measurement reports when an event
  has a reportDescriptor with a specified frequency. Each interval contains
  time-weighted mean power aggregated over the obligation period.

  Background:
    Given I have a VTN token as "bl-client"

  @reporter-resampling @autoretry
  Scenario: Obligation-based report contains multiple intervals
    Given I create a program named "resample-test" and save its ID
    And I create an event for the saved program reporting every 5 seconds
    When I wait for VEN-1 to have at least 1 event
    And I wait for VEN-1 to accumulate at least 20 seconds of history
    And I wait for VEN-1 to submit an obligation-driven report for the event
    Then the latest VEN-1 report for the event has multiple intervals
    And each interval has sequential ids starting from 0
    And each interval contains a USAGE payload
    And each interval contains an OPERATING_STATE payload with value "ACTIVE"

  # D-5/F-9: the timer-driven path is gone. An event that does not ask for
  # reports does not get them — reporting is what `reportDescriptors` requests,
  # and the standing fleet-monitoring event is what keeps an idle fleet visible.
  # This used to assert the opposite (a single-interval timer report), which is
  # why it is inverted here rather than deleted: the contract changed, and the
  # new one deserves the same pinning the old one had.
  @reporter-resampling
  Scenario: An event with no reportDescriptor gets no reports
    Given I create a program named "no-descriptor-test" and save its ID
    And I create an event for the saved program without a reportDescriptor
    When I wait for VEN-1 to have at least 1 event
    Then VEN-1 submits no report for the event within 60 seconds
