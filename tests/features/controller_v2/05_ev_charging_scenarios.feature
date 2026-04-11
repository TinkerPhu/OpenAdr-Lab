Feature: VEN EV Charging Scenarios (Chunk 4)
  Validates EV planning behavior under import capacity constraints
  and user-request paths.

  Background:
    Given the VEN is running with profile "test"

  # ── b) IMPORT_CAPACITY_LIMIT caps EV allocation in plan ─────────────────────
  Scenario: (b) IMPORT_CAPACITY_LIMIT event caps EV allocation in plan slots
    Given I have a VTN token as "any-business"
    And I inject pv irradiance 0.0 via sim inject
    And I inject ev_soc 0.50 via sim inject
    And I inject ev_soc_target 0.90 via sim inject
    And I create a rate-system program and save its ID
    And I POST an EV packet with target_soc 0.90, desired_power_kw 7.0, and latest_end_h 6.0
    And I create an IMPORT_CAPACITY_LIMIT event with limit 5.0 kW for the saved program
    When I wait for the VEN /plan to have a slot with import_cap_kw at most 5.0
    Then all EV allocations in capped slots are at most 5.1 kW

  # ── c) Zero import limit produces zero EV allocation in capped slots ─────────
  Scenario: (c) Zero IMPORT_CAPACITY_LIMIT produces zero EV allocation in capped slots
    Given I have a VTN token as "any-business"
    And I inject pv irradiance 0.0 via sim inject
    And I inject ev_soc 0.50 via sim inject
    And I inject ev_soc_target 0.90 via sim inject
    And I create a rate-system program and save its ID
    And I POST an EV packet with target_soc 0.90, desired_power_kw 7.0, and latest_end_h 6.0
    And I create an IMPORT_CAPACITY_LIMIT event with limit 0.0 kW for the saved program
    When I wait for the VEN /plan to have a slot with import_cap_kw at most 0.1
    Then all EV allocations in capped slots are at most 0.1 kW

  # ── e) User request capped by IMPORT_CAPACITY_LIMIT ─────────────────────────
  Scenario: (e) User request capped by IMPORT_CAPACITY_LIMIT event
    Given I have a VTN token as "any-business"
    And I inject pv irradiance 0.0 via sim inject
    And I inject ev_soc 0.50 via sim inject
    And I inject ev_soc_target 0.90 via sim inject
    And I create a rate-system program and save its ID
    And I POST a user request for EV with target_soc 0.90 and latest_end in 6 hours
    And I create an IMPORT_CAPACITY_LIMIT event with limit 5.0 kW for the saved program
    When I wait for the VEN /plan to have a slot with import_cap_kw at most 5.0
    Then all EV allocations in capped slots are at most 5.1 kW

  # ── f) User request with zero import limit ───────────────────────────────────
  Scenario: (f) User request blocked by zero IMPORT_CAPACITY_LIMIT
    Given I have a VTN token as "any-business"
    And I inject pv irradiance 0.0 via sim inject
    And I inject ev_soc 0.50 via sim inject
    And I inject ev_soc_target 0.90 via sim inject
    And I create a rate-system program and save its ID
    And I POST a user request for EV with target_soc 0.90 and latest_end in 6 hours
    And I create an IMPORT_CAPACITY_LIMIT event with limit 0.0 kW for the saved program
    When I wait for the VEN /plan to have a slot with import_cap_kw at most 0.1
    Then all EV allocations in capped slots are at most 0.1 kW
