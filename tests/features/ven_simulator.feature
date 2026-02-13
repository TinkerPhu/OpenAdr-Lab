Feature: VEN Simulator & Reactor
  The VEN simulator produces physics-based telemetry and the reactor
  responds to OpenADR events by adjusting device setpoints.

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

  Scenario: Trace endpoint returns decision entries
    When I query VEN-1 decision trace
    Then the trace response is a list
    And each trace entry has fields "ts,mode,fsm_state,reason"

  Scenario: Sensor values come from simulator
    When I GET the VEN sensor snapshot
    Then the sensor raw source is "simulator"

  Scenario: Export capacity event triggers reactor response
    Given I have a VTN token as "any-business"
    And I create a program "sim-test-export" targeting "ven-1-name" and save its ID
    When I create a UC event "sim-export-cap" with type "EXPORT_CAPACITY_LIMIT" priority 0 and value 5000
    Then the response status is 201
    When I wait for VEN-1 to show event "sim-export-cap"
    And I wait 5 seconds for the reactor
    And I query VEN-1 decision trace
    Then the trace contains an entry with mode "EXPORT_CAP"

  Scenario: Price event triggers reactor response
    Given I have a VTN token as "any-business"
    And I create a program "sim-test-price" targeting "ven-1-name" and save its ID
    When I create a UC event "sim-price-high" with type "PRICE" priority 5 and value 0.50
    Then the response status is 201
    When I wait for VEN-1 to show event "sim-price-high"
    And I wait 5 seconds for the reactor
    And I query VEN-1 decision trace
    Then the trace contains an entry with mode "PRICE"
