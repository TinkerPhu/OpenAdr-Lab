Feature: VEN EV Charging Scenarios (Chunk 4)
  Validates EV planning behavior under import capacity constraints
  and user-request paths.

  NOTE: Scenarios are ordered so non-zero cap tests run before zero-cap tests.
  The VEN/VTN state persists across scenarios within a feature; running a
  zero-cap event first would leak into subsequent scenarios.

  Background:
    Given the VEN is running with profile "test"

  # ── b) IMPORT_CAPACITY_LIMIT caps net import in plan ─────────────────────────
  Scenario: (b) IMPORT_CAPACITY_LIMIT event caps net import in plan slots
    Given I have a VTN token as "any-business"
    And I inject pv irradiance 0.0 via sim inject
    And I inject ev_soc 0.50 via sim inject
    And I inject ev_soc_target 0.90 via sim inject
    And I create a rate-system program and save its ID
    And I POST an EV session with target_soc 0.90 and departure in 6.0 hours
    And I create an IMPORT_CAPACITY_LIMIT event with limit 5.0 kW for the saved program
    When I wait for the VEN /plan to have a slot with import_cap_kw at most 5.0
    Then net import in all capped plan slots is at most 5.1 kW

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
    Then net import in all capped plan slots is at most 5.1 kW

  # ── c) Zero import cap is acknowledged in plan ───────────────────────────────
  # Under MILP, a MustRun EV with no PV and limited battery cannot avoid all
  # import — the solver minimises violation via soft-constraint slack.  We
  # verify the cap propagates correctly to every slot (Phase 5b will add
  # energy-shortfall slack for a tighter bound).
  Scenario: (c) Zero IMPORT_CAPACITY_LIMIT is reflected in plan slots
    Given I have a VTN token as "any-business"
    And I inject pv irradiance 0.0 via sim inject
    And I inject ev_soc 0.50 via sim inject
    And I inject ev_soc_target 0.90 via sim inject
    And I create a rate-system program and save its ID
    And I POST an EV session with target_soc 0.90 and departure in 6.0 hours
    And I create an IMPORT_CAPACITY_LIMIT event with limit 0.0 kW for the saved program
    When I wait for the VEN /plan to have a slot with import_cap_kw at most 0.1
    Then all capped plan slots have import_cap_kw at most 0.1

  # ── f) User request with zero import limit ───────────────────────────────────
  Scenario: (f) User request with zero IMPORT_CAPACITY_LIMIT is reflected in plan
    Given I have a VTN token as "any-business"
    And I inject pv irradiance 0.0 via sim inject
    And I inject ev_soc 0.50 via sim inject
    And I inject ev_soc_target 0.90 via sim inject
    And I create a rate-system program and save its ID
    And I POST a user request for EV with target_soc 0.90 and latest_end in 6 hours
    And I create an IMPORT_CAPACITY_LIMIT event with limit 0.0 kW for the saved program
    When I wait for the VEN /plan to have a slot with import_cap_kw at most 0.1
    Then all capped plan slots have import_cap_kw at most 0.1
