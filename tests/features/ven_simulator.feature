Feature: VEN Simulator
  The VEN simulator produces physics-based telemetry. OpenADR events are
  processed by the controller (planner + dispatcher) which adjusts device setpoints.

  Scenario: Sim endpoint returns device states and energy counters
    When I query VEN-1 simulator state
    Then the sim response has field "net_power_w"
    And the sim response has field "import_w"
    And the sim response has field "export_w"
    And the sim response has field "voltage_v"
    And the sim response has field "import_kwh"
    And the sim response has field "export_kwh"
    And the sim response has field "ts"

  Scenario: Sim endpoint shows configured devices
    When I query VEN-1 simulator state
    Then the sim response has device "ev"
    And the sim response has device "heater"
    And the sim response has device "pv"

  Scenario: Sensor values come from simulator
    When I wait 3 seconds
    And I GET the VEN sensor snapshot
    Then the sensor raw source is "simulator"

  Scenario: Auto-report submitted for active event
    Given I have a VTN token as "any-business"
    And I create a program "auto-report-test" targeting "ven-1-name" and save its ID
    When I create a UC event "auto-report-evt" with type "IMPORT_CAPACITY_LIMIT" priority 0 and value 5000
    Then the response status is 201
    When I wait for VEN-1 to show event "auto-report-evt"
    And I wait 15 seconds
    Then an auto-report for event "auto-report-evt" exists on VEN-1
