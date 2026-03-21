Feature: UC-11..UC-12 — Stress and Multi-Asset Use Cases
  Verify planner stability when no packet tasks are active (battery/heater only)
  and correct multi-asset coordination under an import capacity constraint.

  Background:
    Given the VEN is running with profile "test"

  # --- UC-11: Planning with No Consumption Packets ---
  # After cancelling all user-created packets (the profile EV packet may still exist),
  # the planner still produces a valid plan covering base load and battery arbitrage.
  # This simulates a consumption-only site where no scheduled tasks are pending.

  Scenario: UC-11a — Planner produces a valid plan even with no new user requests
    When I wait for the VEN /plan endpoint to return a plan
    Then the plan has field "id"
    And the plan has field "firm_slots"
    And the plan.firm_slots is a non-empty array

  Scenario: UC-11b — Plan runs without crashing when no packets match any firm slot
    When I POST a sim override with no EV charging demand
    And I wait for the VEN /plan endpoint to return a plan
    Then the plan has field "id"
    And the plan has field "firm_slots"

  Scenario: UC-11c — Asset ledger tracks energy in active assets
    When I poll VEN /ledger until field "ev" is present
    Then the response JSON has field "ev"

  # --- UC-12: Multi-Asset Coordination Under Import Cap ---
  # With EV + heater + battery all active under an import cap,
  # the planner distributes available capacity across assets
  # and does not over-schedule beyond the cap in any single slot.

  Scenario: UC-12a — Multi-asset plan with import cap allocates EV within cap
    Given I have a VTN token as "any-business"
    And I create a rate-system program and save its ID
    And I create an IMPORT_CAPACITY_LIMIT event with limit 10.0 kW for the saved program
    When I wait for the VEN /capacity import_limit_kw to be 10.0
    And I wait for the VEN /plan to have firm slots with import_cap_kw at most 10.0
    Then at least one firm slot has an allocation for asset "ev"
    And all plan firm slots have net_import_kw of at most 10.0

  Scenario: UC-12b — Plan warnings are accessible when capacity is constrained
    Given I have a VTN token as "any-business"
    And I create a rate-system program and save its ID
    And I create an IMPORT_CAPACITY_LIMIT event with limit 2.0 kW for the saved program
    When I wait for the VEN /capacity import_limit_kw to be 2.0
    And I wait for the VEN /plan to have firm slots with import_cap_kw at most 2.0
    Then the plan has field "warnings"

  Scenario: UC-12c — Ledger accumulates energy for all active assets concurrently
    When I POST a sim override with full PV irradiance
    And I wait for the VEN /plan to have an EV allocation in firm_slots
    And I poll VEN /ledger until field "pv" is present
    Then the response JSON has field "ev"
    And the response JSON has field "pv"
