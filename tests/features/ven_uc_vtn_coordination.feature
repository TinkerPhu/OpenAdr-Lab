Feature: UC-05..UC-07 — VTN Coordination Use Cases
  Verify that the VEN correctly coordinates with the VTN: flexible-horizon envelopes
  are preserved and exposed, grid emergency events constrain the plan, and capacity
  state is accessible via the /capacity endpoint.

  Background:
    Given the VEN is running with profile "test"

  # --- UC-05: Far-Horizon Flexible Pricing ---
  # Energy beyond the near-horizon (4h) is kept FLEXIBLE.
  # FlexibilityEnvelopes are generated for unscheduled packets and exposed via /flexibility.

  Scenario: UC-05a — Plan has FLEXIBLE slots beyond the near-horizon boundary
    When I wait for the VEN /plan endpoint to return a plan
    Then the plan has field "flexible_slots"
    And the plan flexible_slots is a non-empty array

  Scenario: UC-05b — Flexibility envelopes are accessible via GET /flexibility
    Given I have a VTN token as "any-business"
    And I create a rate-system program and save its ID
    And I create a cheap 4-hour PRICE event for the saved program
    When I wait for the VEN /plan to have envelopes
    And I GET /flexibility from the VEN
    Then the response status is 200
    And the response JSON is an array
    And the flexibility envelopes contain an entry for asset "ev"

  Scenario: UC-05c — Each flexibility envelope has energy_needed and rate range fields
    Given I have a VTN token as "any-business"
    And I create a rate-system program and save its ID
    And I create a cheap 4-hour PRICE event for the saved program
    When I wait for the VEN /plan to have envelopes
    Then the first envelope has field "energy_needed_kwh"
    And the first envelope has field "max_acceptable_rate"
    And the first envelope field "energy_needed_kwh" is greater than 0.0

  # --- UC-06: Grid Emergency Alert via Import Capacity Limit ---
  # When the VTN sends an IMPORT_CAPACITY_LIMIT event, the VEN updates
  # its capacity state and the planner restricts import in affected slots.

  Scenario: UC-06a — IMPORT_CAPACITY_LIMIT event updates /capacity import_limit_kw
    Given I have a VTN token as "any-business"
    And I create a rate-system program and save its ID
    And I create an IMPORT_CAPACITY_LIMIT event with limit 3.0 kW for the saved program
    When I wait for the VEN /capacity import_limit_kw to be 3.0
    Then the VEN /capacity response has import_limit_kw equal to 3.0

  Scenario: UC-06b — Plan firm slots respect an import capacity limit
    Given I have a VTN token as "any-business"
    And I create a rate-system program and save its ID
    And I create an IMPORT_CAPACITY_LIMIT event with limit 5.0 kW for the saved program
    When I wait for the VEN /capacity import_limit_kw to be 5.0
    And I wait for the VEN /plan to have firm slots with import_cap_kw at most 5.0
    Then all plan firm slots have import_cap_kw of at most 5.0

  # --- UC-07: Capacity State Accessible ---
  # The VEN exposes its full capacity subscription/reservation/limit state via /capacity.

  Scenario: UC-07 — GET /capacity returns a valid capacity state object
    When I request GET /capacity from the VEN
    Then the response is a JSON object
    And the response contains the field "import_limit_kw"
    And the response contains the field "export_limit_kw"
