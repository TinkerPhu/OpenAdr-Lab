Feature: Hard import limits are met at execution (GB-47)
  An active IMPORT_CAPACITY_LIMIT is met from the next tick whatever the plan
  says: the arbiter's limit-enforcement pass sheds the excess with the
  cheapest lever (battery, EV charging, heater stages) while the planner
  catches up, and each decision is visible in GET /arbiter-diagnostics and
  the controller event log (GET /trace/events). Switching limit enforcement
  off leaves the plan alone — the reference for measuring the planner.

  Background:
    Given I have a VTN token as "any-business"
    And the site has no PV, no EV charging, the heater in its band and the battery at 0.8 SoC

  Scenario: Unforecast load under a capacity limit is shed and traced
    Given limit enforcement is enabled
    And I create an open program "limit-enforcement-on" and save its ID
    And I create a capacity event of type "IMPORT_CAPACITY_LIMIT" with 2.0 kW for the saved program lasting 10 minutes
    When I inject base_load_kw 3.5 with alpha 1.0 via sim inject
    Then the VEN net site power stays at or below 2.05 kW for 15 seconds within 60 seconds
    And the arbiter diagnostics show the limit pass holding import under 2.0 kW with nothing unresolved
    And the controller event log has a "limit" ArbiterDecision
    When I clear the base_load_kw inject
    And I delete the saved capacity event

  Scenario: With limit enforcement off the same load exceeds the limit
    Given limit enforcement is disabled
    And I create an open program "limit-enforcement-off" and save its ID
    And I create a capacity event of type "IMPORT_CAPACITY_LIMIT" with 2.0 kW for the saved program lasting 10 minutes
    When I inject base_load_kw 3.5 with alpha 1.0 via sim inject
    Then the VEN net site power exceeds 2.5 kW within 60 seconds
    When I clear the base_load_kw inject
    And I delete the saved capacity event
