Feature: VEN Entity Model — Stage 1 Foundation
  Verify that all domain entities compile, the Battery actor is present in /sim,
  and HEMS endpoints (/packets, /plan, /tariffs) respond correctly.
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
    When I poll VEN /sim until field "battery.soc" is present
    Then the response JSON field "battery.soc" is a number between 0.0 and 1.0

  Scenario: Battery power_kw is present in sim response
    When I GET /sim from the VEN
    Then the response JSON field "battery.power_kw" is a number between -100.0 and 100.0

  # --- HEMS endpoints (live after Stage 3) ---

  Scenario: GET /packets returns a JSON array
    When I GET /packets from the VEN
    Then the response status is 200
    And the response JSON is an array

  Scenario: GET /tariffs returns a JSON array
    When I GET /tariffs from the VEN
    Then the response status is 200
    And the response JSON is an array

  # --- Backward compatibility ---

  Scenario: Existing /sim endpoint returns structured ts + grid + assets
    When I GET /sim from the VEN
    Then the response status is 200
    And the response JSON has field "ts"
    And the response JSON has field "grid"
    And the response JSON has field "assets"

  Scenario: Existing /health endpoint still works
    When I GET /health from the VEN
    Then the response status is 200

  Scenario: Existing /events endpoint still works
    When I GET /events from the VEN
    Then the response status is 200
    And the response JSON is an array

  Scenario: GET /trace/events endpoint returns a JSON array
    When I GET /trace/events from the VEN
    Then the response status is 200
    And the response JSON is an array

  Scenario: GET /trace/history returns asset history rows for EV
    When I poll VEN /trace/history?asset=ev&limit=5 until it is a non-empty array
    Then the response JSON is an array
