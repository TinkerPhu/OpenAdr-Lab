Feature: VEN EV Charging Scenarios (Chunk 4)
  Validates EV planning behavior under SoC ceiling, import capacity constraints,
  user-request paths, and battery tariff-based decisions.

  Background:
    Given the VEN is running with profile "test"

  # ── a) SoC ceiling blocks charging until target is raised ───────────────────
  Scenario: (a) EV SoC ceiling fires SOC_CEILING then FIRM_OBLIGATION after target raise
    Given I inject ev_soc 0.70 via sim inject
    And I inject ev_soc_target 0.70 via sim inject
    And I POST an EV packet with target_soc 0.85, desired_power_kw 7.0, and latest_end_h 8.0
    When I wait for a "SOC_CEILING" PlanStep for asset "ev"
    When I inject ev_soc_target 0.85 via sim inject
    And I wait for a "FIRM_OBLIGATION" PlanStep for asset "ev"
    Then that PlanStep has setpoint_kw greater than 0.0

  # ── b) IMPORT_CAPACITY_LIMIT caps EV allocation in plan ─────────────────────
  Scenario: (b) IMPORT_CAPACITY_LIMIT event caps EV allocation in plan firm slots
    Given I have a VTN token as "any-business"
    And I inject pv irradiance 0.0 via sim inject
    And I inject ev_soc 0.50 via sim inject
    And I inject ev_soc_target 0.90 via sim inject
    And I create a rate-system program and save its ID
    And I POST an EV packet with target_soc 0.90, desired_power_kw 7.0, and latest_end_h 6.0
    And I create an IMPORT_CAPACITY_LIMIT event with limit 5.0 kW for the saved program
    When I wait for the VEN /plan to have firm slots with import_cap_kw at most 5.0
    And I wait for a "FIRM_OBLIGATION" PlanStep for asset "ev"
    Then at least one PlanStep for asset "ev" has reason kind "FIRM_OBLIGATION"
    And all EV allocations in capped firm slots are at most 5.1 kW

  # ── c) Zero import limit produces zero EV allocation in capped slots ─────────
  Scenario: (c) Zero IMPORT_CAPACITY_LIMIT produces zero EV allocation in capped slots
    Given I have a VTN token as "any-business"
    And I inject pv irradiance 0.0 via sim inject
    And I inject ev_soc 0.50 via sim inject
    And I inject ev_soc_target 0.90 via sim inject
    And I create a rate-system program and save its ID
    And I POST an EV packet with target_soc 0.90, desired_power_kw 7.0, and latest_end_h 6.0
    And I create an IMPORT_CAPACITY_LIMIT event with limit 0.0 kW for the saved program
    When I wait for the VEN /plan to have firm slots with import_cap_kw at most 0.1
    Then all EV allocations in capped firm slots are at most 0.1 kW

  # ── d) User request at SoC ceiling → SOC_CEILING ────────────────────────────
  Scenario: (d) User request blocked by SoC ceiling fires SOC_CEILING
    Given I inject ev_soc 0.70 via sim inject
    And I inject ev_soc_target 0.70 via sim inject
    And I POST a user request for EV with target_soc 0.85 and latest_end in 8 hours
    When I wait for a "SOC_CEILING" PlanStep for asset "ev"

  # ── e) User request capped by IMPORT_CAPACITY_LIMIT ─────────────────────────
  Scenario: (e) User request capped by IMPORT_CAPACITY_LIMIT event
    Given I have a VTN token as "any-business"
    And I inject pv irradiance 0.0 via sim inject
    And I inject ev_soc 0.50 via sim inject
    And I inject ev_soc_target 0.90 via sim inject
    And I create a rate-system program and save its ID
    And I POST a user request for EV with target_soc 0.90 and latest_end in 6 hours
    And I create an IMPORT_CAPACITY_LIMIT event with limit 5.0 kW for the saved program
    When I wait for the VEN /plan to have firm slots with import_cap_kw at most 5.0
    And I wait for a "FIRM_OBLIGATION" PlanStep for asset "ev"
    Then at least one PlanStep for asset "ev" has reason kind "FIRM_OBLIGATION"
    And all EV allocations in capped firm slots are at most 5.1 kW

  # ── f) User request with zero import limit ───────────────────────────────────
  Scenario: (f) User request blocked by zero IMPORT_CAPACITY_LIMIT
    Given I have a VTN token as "any-business"
    And I inject pv irradiance 0.0 via sim inject
    And I inject ev_soc 0.50 via sim inject
    And I inject ev_soc_target 0.90 via sim inject
    And I create a rate-system program and save its ID
    And I POST a user request for EV with target_soc 0.90 and latest_end in 6 hours
    And I create an IMPORT_CAPACITY_LIMIT event with limit 0.0 kW for the saved program
    When I wait for the VEN /plan to have firm slots with import_cap_kw at most 0.1
    Then all EV allocations in capped firm slots are at most 0.1 kW

  # ── g) Battery charges on cheap tariff then discharges on expensive tariff ─
  # The two-pass planner only schedules grid charging when depletion is predicted.
  # Conditions: no_pv_test profile (zero PV forecast), SoC=0.20, 1h cheap then 3h
  # expensive. Shadow sim: battery depletes during the expensive period → cheapest
  # slot before depletion (slot 0, 0.05 EUR/kWh) gets a grid charge → CHEAP_TARIFF.
  Scenario: (g) Battery discharges on expensive tariff and charges on cheap tariff
    Given the VEN is running with profile "no_pv_test"
    And I have a VTN token as "any-business"
    And the battery SoC is reset to 0.20
    And I create a rate-system program and save its ID
    And I create a cheap-then-expensive PRICE event for the saved program
    When I wait for both "EXPENSIVE_TARIFF" and "CHEAP_TARIFF" PlanSteps for asset "battery"
    Then a "EXPENSIVE_TARIFF" PlanStep for "battery" has setpoint_kw less than 0.0
    And a "CHEAP_TARIFF" PlanStep for "battery" has setpoint_kw greater than 0.0
