Feature: Capacity limits apply when they are scheduled (GB-48)
  A capacity limit is a schedule. A limit the VTN announces for later caps
  only the plan slots it covers, so the planner can prepare for it, and it is
  not reported as in force until it starts. A Dynamic Operating Envelope in
  the spec's own form (one event-level intervalPeriod, intervals without their
  own — OpenADR 3.1 User Guide §7.3, Example 8.10.1-1) becomes back-to-back
  intervals.

  Background:
    Given I have a VTN token as "any-business"
    And I create an open program "capacity-schedule-test" and save its ID

  Scenario: A limit announced in advance caps only its slots and is not in force yet
    Given I announce an import capacity limit of 1.5 kW starting in 120 minutes for 60 minutes
    Then the VEN plan caps only the slots overlapping the announced window at 1.5 kW
    And the VEN reports no import limit in force
    When I delete the schedule event

  Scenario: An announced limit comes into force when it starts
    Given I announce an import capacity limit of 1.5 kW starting in 1 minutes for 10 minutes
    Then the VEN reports no import limit in force
    And the VEN reports an import limit of 1.5 kW in force within 150 seconds
    When I delete the schedule event

  Scenario: A spec-form dynamic operating envelope becomes back-to-back intervals
    Given I send a dynamic operating envelope of 8.0 kW then 6.0 kW in contiguous 30-minute intervals starting in 60 minutes
    Then the VEN capacity schedule shows 8.0 kW then 6.0 kW back to back
    When I delete the schedule event
