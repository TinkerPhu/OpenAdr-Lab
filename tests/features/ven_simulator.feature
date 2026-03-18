Feature: VEN Simulator
  The VEN simulator produces physics-based telemetry. OpenADR events are
  processed by the controller (planner + dispatcher) which adjusts device setpoints.

  Scenario: Sim endpoint top-level shape is ts + grid + assets
    When I query VEN-1 simulator state
    Then the sim response top-level keys are "ts,grid,assets"
    And the sim response does not have field "net_power_w"
    And the sim response does not have field "import_w"
    And the sim response does not have field "export_w"
    And the sim response does not have field "ev"
    And the sim response does not have field "heater"
    And the sim response does not have field "pv"
    And the sim response does not have field "battery"
    And the sim response does not have field "base_load_w"

  Scenario: Sim grid object has expected fields
    When I query VEN-1 simulator state
    Then the sim grid has field "net_power_w"
    And the sim grid has field "voltage_v"
    And the sim grid has field "import_kwh"
    And the sim grid has field "export_kwh"
    And the sim grid does not have field "import_w"
    And the sim grid does not have field "export_w"

  Scenario: Sim endpoint shows configured devices
    When I query VEN-1 simulator state
    Then the sim response has device "ev"
    And the sim response has device "heater"
    And the sim response has device "pv"

  Scenario: EV asset has expected fields in sim response
    When I query VEN-1 simulator state
    Then the sim device "ev" has field "power_kw"
    And the sim device "ev" has field "soc"
    And the sim device "ev" has field "soc_pct"
    And the sim device "ev" has field "plugged"
    And the sim device "ev" has field "current_kw"
    And the sim device "ev" has field "max_charge_kw"
    And the sim device "ev" has field "soc_target"
    And the sim device "ev" has field "battery_kwh"

  Scenario: Heater asset has expected fields in sim response
    When I query VEN-1 simulator state
    Then the sim device "heater" has field "power_kw"
    And the sim device "heater" has field "temp_c"
    And the sim device "heater" has field "current_kw"
    And the sim device "heater" has field "max_kw"
    And the sim device "heater" has field "temp_min_c"
    And the sim device "heater" has field "temp_max_c"

  Scenario: PV asset has expected fields in sim response
    When I query VEN-1 simulator state
    Then the sim device "pv" has field "power_kw"
    And the sim device "pv" has field "irradiance"
    And the sim device "pv" has field "current_kw"
    And the sim device "pv" has field "rated_kw"

  Scenario: Battery asset has expected fields in sim response
    When I query VEN-1 simulator state
    Then the sim response has device "battery"
    And the sim device "battery" has field "power_kw"
    And the sim device "battery" has field "soc"
    And the sim device "battery" has field "soc_pct"
    And the sim device "battery" has field "current_kw"
    And the sim device "battery" has field "capacity_kwh"
    And the sim device "battery" has field "max_charge_kw"
    And the sim device "battery" has field "max_discharge_kw"
    And the sim device "battery" has field "min_soc"

  Scenario: Base load asset has expected fields in sim response
    When I query VEN-1 simulator state
    Then the sim response has device "base_load"
    And the sim device "base_load" has field "power_kw"
    And the sim device "base_load" has field "current_kw"

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
