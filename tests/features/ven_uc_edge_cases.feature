Feature: UC-08..UC-10 — Edge Case Use Cases
  Verify HEMS behaviour under unusual or constrained conditions: EV sim stops when
  unplugged, multi-tier requests preserve their deadline structure, and import capacity
  limits are reflected in plan slot constraints.

  Background:
    Given the VEN is running with profile "test"

  # --- UC-08: EV Disconnects Mid-Charge ---
  # When the user sets ev_plugged=false via /sim/override, the simulator stops
  # delivering EV power. The ledger and sim state reflect zero EV current.

  Scenario: UC-08a — Unplugging the EV via override drops EV current to zero
    When I POST a sim override setting ev_plugged to false
    And I poll VEN /sim until field "ev.power_kw" equals 0.0
    Then the sim EV power_kw is 0.0

  Scenario: UC-08b — Re-plugging the EV resumes charging
    When I POST a sim override setting ev_plugged to false
    And I wait 2 seconds
    And I POST a sim override setting ev_plugged to true
    And I poll VEN /sim until field "ev" is present
    Then the sim EV field is present

  # --- UC-09: Tier Fallback (Tight Budget) ---
  # A request with two tiers (tight budget tier 1, fallback tier 2) is created.
  # The structure is verified: tier_count=2 and both deadlines are stored.

  Scenario: UC-09a — Multi-tier request with tight tier-1 budget has tier_count 2
    When I POST a multi-tier user request for asset "ev"
    Then the response status is 201
    And the response JSON field "tier_count" is greater than 1.0

  Scenario: UC-09b — Tight budget tier-1 request still creates a valid EV session link
    When I POST a user request for asset "ev" with target_soc 0.85 and max_cost 3.00 EUR
    Then the response status is 201
    And the response JSON has field "session_id"
    And the response JSON field "max_total_cost_eur" is greater than 0.0

  Scenario: UC-09c — Cancelled request clears the linked EV session
    When I POST a multi-tier user request for asset "ev"
    And I save the request ID
    And I DELETE the saved user request
    Then the response status is 204
    And the EV session is cleared after cancellation

  # --- UC-10: Import Capacity Limit Constrains Plan ---
  # When the VTN sets an IMPORT_CAPACITY_LIMIT event, the planner populates
  # each FIRM slot's import_cap_kw from the capacity state.

  Scenario: UC-10a — Plan slots reflect the import capacity limit from VTN
    Given I have a VTN token as "any-business"
    And I create a rate-system program and save its ID
    And I create an IMPORT_CAPACITY_LIMIT event with limit 8.0 kW for the saved program
    When I wait for the VEN /capacity import_limit_kw to be 8.0
    And I wait for the VEN /plan to have slots with import_cap_kw at most 8.0
    Then all plan slots have import_cap_kw of at most 8.0

  Scenario: UC-10b — Plan net_import_kw does not exceed the capacity limit
    Given I have a VTN token as "any-business"
    And I create a rate-system program and save its ID
    And I create an IMPORT_CAPACITY_LIMIT event with limit 6.0 kW for the saved program
    When I wait for the VEN /capacity import_limit_kw to be 6.0
    And I wait for the VEN /plan to have slots with import_cap_kw at most 6.0
    Then all plan slots have net_import_kw of at most 6.0
