Feature: Hard import limits are met at execution (GB-47)
  An active import limit is met from the next tick whatever the plan says:
  the arbiter's limit-enforcement pass sheds the excess with the cheapest
  lever (battery, EV charging, heater stages) while the planner catches up,
  and each decision is visible in GET /arbiter-diagnostics and the
  controller event log (GET /trace/events). Switching limit enforcement off
  leaves the plan alone — the reference for measuring the planner.

  Background:
    Given I have a VTN token as "any-business"
    And the site has no PV, no EV charging, the heater in its band and the battery at 0.8 SoC

  Scenario: A VTN capacity limit reaches the limit pass and site import stays under it
    Given limit enforcement is enabled
    And I create an open program "limit-enforcement-vtn" and save its ID
    And I create a capacity event of type "IMPORT_CAPACITY_LIMIT" with 2.0 kW for the saved program lasting 10 minutes
    When I wait for the VEN /plan to have at least one slot with import_cap_kw at most 2.0
    And a sustained base-load step of 3.5 kW starts
    Then the VEN net site power stays at or below 2.05 kW for 15 seconds within 60 seconds
    And the arbiter diagnostics show the limit pass steering under 2.0 kW with nothing unresolved
    When I delete the saved capacity event

  # The simulator's injected import limit is seen only by execution, never by
  # the planner, so the plan keeps violating it: only the limit pass can hold it.
  Scenario: A limit the plan does not know is shed by the limit pass and traced
    Given limit enforcement is enabled
    And the simulator imposes an import limit of 2.0 kW
    When a sustained base-load step of 3.5 kW starts
    Then the VEN net site power stays at or below 2.05 kW for 15 seconds within 60 seconds
    And the arbiter diagnostics show the limit pass steering under 2.0 kW with a lever and nothing unresolved
    And the controller event log has a "limit" ArbiterDecision

  Scenario: With limit enforcement off the same load exceeds the limit
    Given limit enforcement is disabled
    And the simulator imposes an import limit of 2.0 kW
    When a sustained base-load step of 3.5 kW starts
    Then the VEN net site power exceeds 2.5 kW within 60 seconds
