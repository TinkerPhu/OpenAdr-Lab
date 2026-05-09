Feature: VEN Dispatcher — Stage 4 (Plan Execution + Asset Ledger)
  Verify that the Dispatcher executes plan slot allocations: the EV sim
  charges when planned, EV sessions trigger replanning, and the asset ledger
  accumulates energy over time.
  Verify that Layer 1 battery correction reacts to grid deviations immediately.

  Background:
    Given the VEN is running with profile "test"

  # --- EV session drives plan allocation ---

  Scenario: EV session drives dispatcher to allocate power to EV
    Given I inject ev_soc 0.50 via sim inject
    And I POST an EV session with target_soc 0.90 and departure in 12.0 hours
    When I wait for the VEN /plan to have an EV allocation in slots
    Then at least one firm slot has an allocation for asset "ev"

  # --- EV session CRUD via /ev-session ---

  Scenario: POST /ev-session creates a new EvSession
    When I POST an EV session with target_soc 0.95 and departure in 12.0 hours
    Then the response status is 201
    And the response JSON has field "id"
    And the response JSON has field "target_soc"

  Scenario: EvSession appears in GET /ev-session after POST
    When I POST an EV session with target_soc 0.95 and departure in 12.0 hours
    And I GET the EV session from /ev-session
    Then the response status is 200
    And the response JSON has field "id"

  # --- Asset energy ledger ---

  Scenario: GET /ledger returns per-asset energy accumulation after charging
    Given I inject ev_soc 0.5 via sim inject
    And I POST an EV session with target_soc 0.90 and departure in 12.0 hours
    When I wait for the VEN /plan to have an EV allocation in slots
    And I poll VEN /ledger until field "ev.energy_kwh" is greater than 0.0
    Then the response JSON has field "ev"
    And the response JSON field "ev.energy_kwh" is greater than 0.0

  # --- Layer 1: reactive battery correction ---
  # Verifies that apply_battery_correction_overlay reacts within one tick when
  # the grid deviates beyond the threshold, and holds the corrected setpoint.

  Scenario: Layer 1 corrects grid deviation immediately using battery
    Given the battery SoC is reset to 0.5
    When I wait for the VEN /plan endpoint to return a plan
    And I inject base_load_kw 10.0 with alpha 0.0 via sim inject
    Then within 5 seconds the VEN sim battery power_kw is less than -1.0

