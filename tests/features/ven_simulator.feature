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
    And the sim device "ev" has field "plugged"
    And the sim device "ev" has field "max_charge_kw"
    And the sim device "ev" has field "soc_target"
    And the sim device "ev" has field "battery_kwh"

  Scenario: Heater asset has expected fields in sim response
    When I query VEN-1 simulator state
    Then the sim device "heater" has field "power_kw"
    And the sim device "heater" has field "temp_c"
    And the sim device "heater" has field "max_kw"
    And the sim device "heater" has field "temp_min_c"
    And the sim device "heater" has field "temp_max_c"

  Scenario: PV asset has expected fields in sim response
    When I query VEN-1 simulator state
    Then the sim device "pv" has field "power_kw"
    And the sim device "pv" has field "irradiance"
    And the sim device "pv" has field "rated_kw"

  Scenario: Battery asset has expected fields in sim response
    When I query VEN-1 simulator state
    Then the sim response has device "battery"
    And the sim device "battery" has field "power_kw"
    And the sim device "battery" has field "soc"
    And the sim device "battery" has field "capacity_kwh"
    And the sim device "battery" has field "max_charge_kw"
    And the sim device "battery" has field "max_discharge_kw"
    And the sim device "battery" has field "min_soc"

  Scenario: Base load asset has expected fields in sim response
    When I query VEN-1 simulator state
    Then the sim response has device "base_load"
    And the sim device "base_load" has field "power_kw"

  Scenario: Sensor values come from simulator
    When I poll VEN /sensors until field "raw.source" is present
    And I GET the VEN sensor snapshot
    Then the sensor raw source is "simulator"

  Scenario: Heater sim schema exposes all four controls
    When I query the VEN-1 sim schema
    Then the schema for "heater" has control key "heater_setpoint_c"
    And the schema for "heater" has control key "heater_temp_c"
    And the schema for "heater" has control key "heater_temp_min_c"
    And the schema for "heater" has control key "heater_temp_max_c"

  # The VEN reports for an active event without anyone POSTing a report — the
  # scenario's original point. It used to be the timer path that did this, for
  # any active event; since D-5/F-9 that path is gone and the obligation loop
  # does it for events that *ask*, via reportDescriptors. Same guarantee,
  # asked for the way the spec asks for it.
  Scenario: A VEN reports unprompted for an event that asked for reports
    Given I have a VTN token as "bl-client"
    And I create a program named "auto-report-test" and save its ID
    And I create an event for the saved program reporting every 5 seconds
    # Discovery first, on its own budget: the VEN polls events every 30 s, so a
    # report-wait that starts before the VEN has even seen the event is spending
    # most of its timeout on the wrong wait -- which is exactly how this
    # scenario failed when it was first rewritten.
    When I wait for VEN-1 to have at least 1 event
    And I wait for VEN-1 to submit an obligation-driven report for the event
    # The claim is that a report arrives unasked-for, not what shape it has. How
    # many intervals it carries depends on how much history the resampler had,
    # which is a race this scenario has no reason to run.
    Then the latest VEN-1 report for the event has a "USAGE" payload with a non-negative number value
