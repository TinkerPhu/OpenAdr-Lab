Feature: Running without planning (opportunistic objective, UC-19)
  Under the `opportunistic` objective the VEN dispatches like the common non-planning HEMS:
  each tick it shares the site balance out to the assets in their declared order (a PV
  surplus is absorbed, storage covers import, nothing is exported from the battery), requests
  are served without look-ahead, and the plan it adopts is a forecast of exactly that
  (status SIMULATED). Switching back to a planning objective lets the planner's allocations
  drive the assets again. The objective and who set it are on GET /plan/objective.

  Scenario: The VEN UI switches to the opportunistic objective and the VEN adopts a forecast
    When I set the VEN's objective to "opportunistic"
    Then GET /plan/objective reports "opportunistic" set by "ven_ui"
    And within 90 seconds the VEN adopts a "SIMULATED" plan under "opportunistic" with slots

  # The ranked assets (EV, heater, shiftable loads, battery, in the order each declares; pinned by
  # `controller/arbiter/tests/opportunistic_tests.rs`) take the whole surplus between them, and
  # the battery takes what the heater leaves. How much the heater takes depends on its own state
  # (a heater whose ceiling latch is set takes nothing), so the scenario asserts the sum. PV comes
  # from a published measurement, which outranks the PV model. A released irradiance inject leaves
  # a decaying offset (time constant about 47 min) that adds to the measurement too, whichever
  # scenario released it, so this one flushes it first: by day an earlier "no PV" turned the
  # 5 kW into 2.7 kW, and at midday into 0.3 kW.
  Scenario: A PV surplus is absorbed by the ranked assets instead of being exported
    Given the VEN-1 pv irradiance offset is flushed to zero
    And the site has no EV charging, the heater in its band and the battery at 0.5 SoC
    And the VEN runs under the "opportunistic" objective
    And a PV measurement of 5 kW is published to the test Mosquitto broker for VEN-1
    Then within 30 seconds the "heater" and the "battery" together draw at least 4 kW
    And within 30 seconds the "battery" draws at least 1 kW
    And the VEN net site power stays at or below 0.5 kW for 10 seconds within 30 seconds

  Scenario: The battery covers import and never exports
    Given the site has no PV, no EV charging, the heater in its band and the battery at 0.8 SoC
    And the VEN runs under the "opportunistic" objective
    When a sustained base-load step of 2.0 kW starts
    Then the VEN net site power stays between -0.2 and 0.3 kW for 10 seconds within 45 seconds

  Scenario: A BY_DEADLINE EV request charges at once, without look-ahead
    Given the site has no PV, the EV plugged at 0.3 SoC, the heater in its band and the battery at 0.1 SoC
    And the VEN runs under the "opportunistic" objective
    When I POST a mode EV session with mode "BY_DEADLINE" and no budget
    Then within 30 seconds the "ev" draws at least 2 kW

  Scenario: An OPPORTUNISTIC EV request waits for a surplus
    Given the site has no PV, the EV plugged at 0.3 SoC, the heater in its band and the battery at 0.1 SoC
    And the VEN runs under the "opportunistic" objective
    When I POST a mode EV session with mode "OPPORTUNISTIC" and no budget
    Then the "ev" stays at or below 0.1 kW for 20 seconds within 40 seconds

  Scenario: An import limit bounds a start-now EV request
    Given limit enforcement is enabled
    And the site has no PV, the EV plugged at 0.3 SoC, the heater in its band and the battery at 0.1 SoC
    And the simulator imposes an import limit of 3.0 kW
    And the VEN runs under the "opportunistic" objective
    When I POST a mode EV session with mode "BY_DEADLINE" and no budget
    Then the VEN net site power stays at or below 3.05 kW for 15 seconds within 60 seconds

  Scenario: Switching back to a planning objective lets the plan drive again
    Given I set the VEN's objective to "opportunistic"
    And within 90 seconds the VEN adopts a "SIMULATED" plan under "opportunistic" with slots
    When I set the VEN's objective to "min_cost"
    Then GET /plan/objective reports "min_cost" set by "ven_ui"
    And within 120 seconds the VEN adopts a solved plan under "min_cost"
