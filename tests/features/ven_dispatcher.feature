Feature: VEN Dispatcher — Stage 4 (Plan Execution + Asset Ledger)
  Verify that the Dispatcher executes plan slot allocations: the EV sim
  charges when planned, EV sessions trigger replanning, and the asset ledger
  accumulates energy over time.

  Background:
    Given the VEN is running with profile "test"

  # --- EV session drives plan allocation ---

  Scenario: EV session drives dispatcher to allocate power to EV
    Given I inject ev_soc 0.50 via sim inject
    And I POST an EV session with target_soc 0.60 and departure in 1.5 hours
    When I wait for the VEN /plan to have an EV allocation in slots
    Then at least one firm slot has an allocation for asset "ev"

  # --- EV session CRUD via /ev-session ---

  Scenario: POST /ev-session creates a new EvSession
    When I POST an EV session with target_soc 0.95 and departure in 1.5 hours
    Then the response status is 201
    And the response JSON has field "id"
    And the response JSON has field "target_soc"

  Scenario: EvSession appears in GET /ev-session after POST
    When I POST an EV session with target_soc 0.95 and departure in 1.5 hours
    And I GET the EV session from /ev-session
    Then the response status is 200
    And the response JSON has field "id"

  # --- Asset energy ledger ---

  Scenario: GET /ledger returns per-asset energy accumulation after charging
    Given I inject ev_soc 0.5 via sim inject
    And I POST an EV session with target_soc 0.60 and departure in 1.5 hours
    When I wait for the VEN /plan to have an EV allocation in slots
    And I poll VEN /ledger until field "ev.energy_kwh" is greater than 0.0
    Then the response JSON has field "ev"
    And the response JSON field "ev.energy_kwh" is greater than 0.0
