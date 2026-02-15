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
    When I wait 3 seconds for the reactor
    And I GET the VEN sensor snapshot
    Then the sensor raw source is "simulator"

  Scenario: Export capacity event triggers reactor response
    Given I have a VTN token as "any-business"
    And I create a program "sim-test-export" targeting "ven-1-name" and save its ID
    When I create a short-lived UC event "sim-export-cap" with type "EXPORT_CAPACITY_LIMIT" priority 0 and value 5000
    Then the response status is 201
    When I wait for VEN-1 to show event "sim-export-cap"
    And I wait 5 seconds for the reactor
    And I query VEN-1 decision trace
    Then the trace contains an entry with mode "EXPORT_CAP"
    # Wait for the short-lived event to expire so it doesn't interfere with the next scenario
    When I wait 15 seconds for the reactor

  Scenario: Price event triggers reactor response
    Given I have a VTN token as "any-business"
    And I create a program "sim-test-price" targeting "ven-1-name" and save its ID
    When I create a short-lived UC event "sim-price-high" with type "PRICE" priority 5 and value 0.50
    Then the response status is 201
    When I wait for VEN-1 to show event "sim-price-high"
    And I wait 5 seconds for the reactor
    And I query VEN-1 decision trace
    Then the trace contains an entry with mode "PRICE"

  Scenario: Simple curtail event triggers reactor response
    Given I have a VTN token as "any-business"
    And I create a program "sim-test-simple" targeting "ven-1-name" and save its ID
    When I create a short-lived UC event "sim-simple-curtail" with type "SIMPLE" priority 0 and value 0
    Then the response status is 201
    When I wait for VEN-1 to show event "sim-simple-curtail"
    And I wait 5 seconds for the reactor
    And I query VEN-1 decision trace
    Then the trace contains an entry with mode "SIMPLE"

  Scenario: Per-interval timing — only current interval drives reactor
    Given I have a VTN token as "any-business"
    And I create a program "sim-test-timed" targeting "ven-1-name" and save its ID
    # Create 3-interval event: first interval active now (EXPORT_CAPACITY_LIMIT 100/50/100)
    When I create a timed UC event "sim-timed-intervals" with type "EXPORT_CAPACITY_LIMIT" priority 0 and 3 intervals
    Then the response status is 201
    When I wait for VEN-1 to show event "sim-timed-intervals"
    And I wait 5 seconds for the reactor
    And I query VEN-1 decision trace
    # The first interval (value 100.0) should be active now
    Then the trace contains an entry with mode "EXPORT_CAP"

  Scenario: Auto-report submitted for active event
    Given I have a VTN token as "any-business"
    And I create a program "auto-report-test" targeting "ven-1-name" and save its ID
    When I create a UC event "auto-report-evt" with type "IMPORT_CAPACITY_LIMIT" priority 0 and value 5000
    Then the response status is 201
    When I wait for VEN-1 to show event "auto-report-evt"
    And I wait 15 seconds for the reactor
    Then an auto-report for event "auto-report-evt" exists on VEN-1
