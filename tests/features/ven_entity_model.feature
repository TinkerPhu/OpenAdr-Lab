Feature: VEN Entity Model — Stage 1 Foundation
  Verify that all domain entities compile, the Battery actor is present in /sim,
  and HEMS endpoints (/packets, /plan, /rates) respond correctly.
  Existing endpoints remain backward-compatible.

  Background:
    Given the VEN is running with profile "test"

  # --- Battery actor in /sim ---

  Scenario: GET /sim includes battery field when battery is configured in profile
    When I GET /sim from the VEN
    Then the response status is 200
    And the response JSON has field "battery"
    And the response JSON field "battery.soc" is a number between 0.0 and 1.0
    And the response JSON field "battery.capacity_kwh" is greater than 0.0
    And the response JSON field "battery.max_charge_kw" is greater than 0.0
    And the response JSON field "battery.max_discharge_kw" is greater than 0.0

  Scenario: Battery SoC stays within valid range over time
    When I wait 3 seconds
    And I GET /sim from the VEN
    Then the response JSON field "battery.soc" is a number between 0.0 and 1.0

  Scenario: Battery current_kw is 0.0 when no dispatcher is active (Stage 1 hold)
    When I GET /sim from the VEN
    Then the response JSON field "battery.current_kw" equals 0.0

  # --- HEMS endpoints (live after Stage 3) ---

  Scenario: GET /packets returns a JSON array
    When I GET /packets from the VEN
    Then the response status is 200
    And the response JSON is an array

  Scenario: GET /rates returns a JSON array
    When I GET /rates from the VEN
    Then the response status is 200
    And the response JSON is an array

  # --- Backward compatibility ---

  Scenario: Existing /sim endpoint still returns all legacy fields
    When I GET /sim from the VEN
    Then the response status is 200
    And the response JSON has field "net_power_w"
    And the response JSON has field "import_w"
    And the response JSON has field "export_w"
    And the response JSON has field "import_kwh"
    And the response JSON has field "export_kwh"
    And the response JSON has field "ev"
    And the response JSON has field "pv"

  Scenario: Existing /health endpoint still works
    When I GET /health from the VEN
    Then the response status is 200

  Scenario: Existing /events endpoint still works
    When I GET /events from the VEN
    Then the response status is 200
    And the response JSON is an array

  Scenario: Existing /trace endpoint still works
    When I GET /trace from the VEN
    Then the response status is 200
    And the response JSON is an array
