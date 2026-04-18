Feature: UC-05..UC-07 — VTN Coordination Use Cases
  Verify that the VEN correctly coordinates with the VTN: flexible-horizon envelopes
  are preserved and exposed, grid emergency events constrain the plan, and capacity
  state is accessible via the /capacity endpoint.

  Background:
    Given the VEN is running with profile "test"

  # --- UC-05: Far-Horizon Flexible Pricing ---
  # FlexibilityEnvelopes are generated for unscheduled packets and exposed via /flexibility.

  Scenario: UC-05a — Plan has slots covering the planning horizon
    When I wait for the VEN /plan endpoint to return a plan
    Then the plan has field "slots"
    And the plan.slots is a non-empty array

  Scenario: UC-05c — Each flexibility envelope in /plan has energy_needed and rate range fields
    Given I inject ev_soc 0.5 via sim inject
    And I POST an EV session with target_soc 0.90 and departure in 12 hours
    And I have a VTN token as "any-business"
    And I create a rate-system program and save its ID
    And I create a cheap 4-hour PRICE event for the saved program
    When I wait for the VEN /plan to have envelopes
    Then the first envelope has field "energy_needed_kwh"
    And the first envelope has field "max_acceptable_rate"
    And the first envelope field "energy_needed_kwh" is greater than 0.0

  @phase-e
  Scenario: UC-05d — GET /flexibility returns site-level headroom with correct shape
    When I GET /flexibility from the VEN
    Then the response status is 200
    And the response JSON contains field "up_kw"
    And the response JSON contains field "down_kw"
    And the response JSON contains field "ts"
    And the response JSON field "down_kw" is greater than 0.0

  # --- UC-06: Grid Emergency Alert via Import Capacity Limit ---
  # When the VTN sends an IMPORT_CAPACITY_LIMIT event, the VEN updates
  # its capacity state and the planner restricts import in affected slots.

  Scenario: UC-06a — IMPORT_CAPACITY_LIMIT event updates /capacity import_limit_kw
    Given I have a VTN token as "any-business"
    And I create a rate-system program and save its ID
    And I create an IMPORT_CAPACITY_LIMIT event with limit 3.0 kW for the saved program
    When I wait for the VEN /capacity import_limit_kw to be 3.0
    Then the VEN /capacity response has import_limit_kw equal to 3.0

  Scenario: UC-06b — Plan slots respect an import capacity limit
    Given I have a VTN token as "any-business"
    And I create a rate-system program and save its ID
    And I create an IMPORT_CAPACITY_LIMIT event with limit 5.0 kW for the saved program
    When I wait for the VEN /capacity import_limit_kw to be 5.0
    And I wait for the VEN /plan to have slots with import_cap_kw at most 5.0
    Then all plan slots have import_cap_kw of at most 5.0

  # --- UC-07: Capacity State Accessible ---
  # The VEN exposes its full capacity subscription/reservation/limit state via /capacity.

  Scenario: UC-07 — GET /capacity returns a valid capacity state object
    When I request GET /capacity from the VEN
    Then the response is a JSON object
    And the response contains the field "import_limit_kw"
    And the response contains the field "export_limit_kw"
