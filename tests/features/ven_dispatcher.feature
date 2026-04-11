Feature: VEN Dispatcher — Stage 4 (Plan Execution + Asset Ledger)
  Verify that the Dispatcher executes plan slot allocations: the EV sim
  charges when planned, packets transition to ACTIVE, POST /packets creates
  schedulable tasks, and the asset ledger accumulates energy over time.

  Background:
    Given the VEN is running with profile "test"

  # --- Packet status lifecycle ---

  Scenario: EV packet transitions to ACTIVE once the dispatcher starts commanding power
    When I wait for the VEN /plan to have an EV allocation in slots
    And I poll VEN /packets until asset "ev" has status "ACTIVE"
    Then the response JSON is an array
    And at least one packet has asset_id "ev"
    And at least one packet with asset_id "ev" has status "ACTIVE"

  # --- POST /packets endpoint ---

  Scenario: POST /packets creates a new EnergyPacket
    When I POST a new EV packet with target_soc 0.95 to /packets
    Then the response status is 201
    And the response JSON has field "id"
    And the response JSON field "asset_id" is the string "ev"
    And the response JSON field "status" is the string "PENDING"

  Scenario: New packet appears in GET /packets after POST
    When I POST a new EV packet with target_soc 0.95 to /packets
    And I GET /packets from the VEN
    Then the response JSON is an array
    And the packets list has at least 1 item

  # --- Asset energy ledger ---

  Scenario: GET /ledger returns per-asset energy accumulation after charging
    When I wait for the VEN /plan to have an EV allocation in slots
    And I poll VEN /ledger until field "ev.energy_kwh" is greater than 0.0
    Then the response JSON has field "ev"
    And the response JSON field "ev.energy_kwh" is greater than 0.0
