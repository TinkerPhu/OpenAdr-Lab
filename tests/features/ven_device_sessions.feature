Feature: Device Sessions API — EV, Heater, Shiftable Loads
  Verify CRUD operations and plan integration for the device-centric
  session endpoints introduced in Phase A/B: EvSession, HeaterTarget,
  and ShiftableLoad.

  Background:
    Given the VEN is running with profile "test"

  # ── EV Session CRUD ────────────────────────────────────────────────────────

  Scenario: POST /ev-session creates an EV session
    When I POST an EV session with target_soc 0.90 and departure in 1.5 hours
    Then the response status is 201
    And the response JSON has field "id"
    And the response JSON has field "target_soc"
    And the response JSON has field "departure_time"

  Scenario: GET /ev-session returns the active session after POST
    Given I POST an EV session with target_soc 0.85 and departure in 1.5 hours
    When I GET the EV session from /ev-session
    Then the response status is 200
    And the response JSON has field "id"
    And the response JSON has field "target_soc"

  Scenario: DELETE /ev-session removes the session
    Given I POST an EV session with target_soc 0.80 and departure in 1.5 hours
    When I DELETE the EV session
    Then the response status is 204

  # ── EV Session plan integration ────────────────────────────────────────────

  Scenario: EV session drives the planner to allocate EV charging power
    Given I inject ev_soc 0.50 via sim inject
    And I POST an EV session with target_soc 0.90 and departure in 1.5 hours
    When I wait for the VEN /plan to have an EV allocation in slots
    Then at least one firm slot has an allocation for asset "ev"

  # ── Heater Target CRUD ─────────────────────────────────────────────────────

  Scenario: POST /heater-target creates a heater target
    When I POST a heater target of 55.0 C ready in 1.5 hours
    Then the response status is 201
    And the response JSON has field "id"
    And the response JSON has field "target_temp_c"
    And the response JSON has field "ready_by"

  Scenario: DELETE /heater-target clears the target
    Given I POST a heater target of 60.0 C ready in 1.5 hours
    When I DELETE the heater target
    Then the response status is 204

  # ── Shiftable Load CRUD ────────────────────────────────────────────────────

  Scenario: POST /shiftable-loads adds a shiftable load
    When I POST a shiftable load for asset "wm" at 2.0 kW for 90 minutes within 2 hours
    Then the response status is 201
    And the response JSON has field "id"
    And the response JSON has field "asset_id"
    And the response JSON has field "power_kw"

  Scenario: DELETE /shiftable-loads/{id} removes the load
    Given I POST a shiftable load for asset "wm" at 2.0 kW for 60 minutes within 2 hours
    When I DELETE shiftable load with saved id
    Then the response status is 204
