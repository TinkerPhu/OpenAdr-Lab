Feature: Multi-asset deviation absorber (Tier 1 real-time control)

  Background:
    Given the VEN is running with the test profile
    And the absorber is enabled

  # User Story 1: Absorber Absorbs Transient Deviations
  # =====================================================

  Scenario: Battery absorbs positive deviation within capacity
    Given the battery SoC is reset to 0.50
    And the plan state is initialized with net import 0.0 kW
    When I inject a PV drop of 2.0 kW (positive deviation)
    And I wait 2 ticks for the sim to process
    Then the battery setpoint is more negative than -1.5 kW
    And the absorber residual is less than 0.2 kW
    And no DeviceDeviation trigger has fired within 30 ticks

  Scenario: EV absorbs residual when battery at floor
    Given the battery SoC is reset to min_soc
    And the EV is plugged with SoC at 0.30
    And the plan state is initialized with net import 0.0 kW
    When I inject a PV drop of 4.0 kW (positive deviation)
    And I wait 2 ticks for the sim to process
    Then the battery setpoint is at max discharge
    And the EV charge setpoint is more negative than baseline
    And the absorber residual is less than 1.0 kW
    And no DeviceDeviation trigger has fired within 30 ticks

  Scenario: Dead-band prevents correction on small deviations
    Given the battery SoC is reset to 0.50
    And the plan state is initialized with net import 0.0 kW
    When I inject a PV drop of 0.05 kW (small positive deviation within dead-band)
    And I wait 1 tick for the sim to process
    Then the battery setpoint is unchanged
    And the absorber residual equals the injected deviation
    And correction_is_active is false

  Scenario: Settling ramps overlay to zero when deviation clears
    Given the battery SoC is reset to 0.50
    And the plan state is initialized with net import 0.0 kW
    When I inject a PV drop of 1.0 kW (positive deviation)
    And I wait 2 ticks for the sim to process
    Then the battery setpoint is negative
    And the absorber is active with an overlay
    When I clear the deviation injection
    And I wait 2 ticks for the sim to process
    Then the battery setpoint returns to near 0.0 kW
    And the absorber settling counter increments
    And the overlay goes to zero

  # User Story 2: Relay Wear Protection via Linger Enforcement
  # ===========================================================

  Scenario: Heater linger prevents rapid relay switching
    Given the heater is configured with min_state_linger_s of 5 seconds
    And the battery SoC is reset to min_soc
    And the EV SoC is reset to soc_target
    And the plan state is initialized with net import 0.0 kW
    When I inject a sustained negative deviation of -2.0 kW (surplus absorption)
    And I wait 2 ticks for the sim to process
    Then the heater setpoint has changed
    And the absorber last_state_change_ts is recorded for heater
    When I inject another negative deviation of -2.0 kW immediately after
    And I wait 1 tick for the sim to process
    Then the heater setpoint does not change again
    And the absorber residual propagates uncovered
    When I wait 5 seconds for the linger window to elapse
    And I inject another negative deviation of -2.0 kW
    And I wait 1 tick for the sim to process
    Then the heater setpoint can change again

  # User Story 3: EV Departure Guard
  # ================================

  Scenario: EV departure guard prevents reduction near departure
    Given the EV is configured with departure in 20 minutes
    And the EV is plugged with SoC at 0.30 (below target)
    And the ev_departure_guard_s is set to 1800 seconds (30 minutes)
    And the battery SoC is reset to 0.50
    And the plan state is initialized with net import 0.0 kW
    When I inject a PV drop of 3.0 kW (positive deviation)
    And I wait 2 ticks for the sim to process
    Then the absorber skips the EV asset
    And the battery setpoint is more negative than -2.5 kW
    And the EV charge setpoint is unchanged from baseline

  Scenario: EV allowed to absorb surplus near departure
    Given the EV is configured with departure in 20 minutes
    And the EV is plugged with SoC at 0.30 (below target)
    And the ev_departure_guard_s is set to 1800 seconds (30 minutes)
    And the battery SoC is reset to 0.50
    And the plan state is initialized with net import 0.0 kW
    When I inject PV surplus of -2.0 kW (negative deviation)
    And I wait 2 ticks for the sim to process
    Then the absorber can adjust the EV charging
    And the EV charge setpoint is more positive than baseline
    And the EV moves closer to soc_target

  # User Story 4: Tier 2 Escalation with Improved Gate
  # ===================================================

  Scenario: DeviceDeviation fires when absorber residual sustained
    Given the battery SoC is reset to min_soc
    And the EV is plugged with SoC at soc_target
    And the heater is at temp_max_c
    And the plan state is initialized with net import 0.0 kW
    And all absorber assets are at or near their limits
    When I inject a sustained positive deviation of 5.0 kW
    And I wait for deviation_trigger_ticks ticks
    Then the DeviceDeviation trigger fires
    And a new MILP plan is produced
    And the replanning is triggered only once (no chattering)

  Scenario: DeviceDeviation does not fire for transient deviations
    Given the battery SoC is reset to 0.50
    And the plan state is initialized with net import 0.0 kW
    When I inject a positive deviation of 2.0 kW
    And I wait 2 ticks for the sim to process
    And the deviation is absorbed by the battery
    Then no DeviceDeviation trigger fires within 120 ticks
    And the MILP planner does not execute a replan
