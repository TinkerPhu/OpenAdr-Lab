Feature: Multi-asset deviation absorber — isolated scenarios
  # Timing note: requires a fresh plan after EV session and deviation injection.
  # Passes in isolation (~19s) but can exceed poll_until timeouts at the end of
  # the full suite on Pi4 when preceding scenarios leave the planner backlogged.

  Background:
    Given the VEN is running with the test profile
    And the absorber is enabled
    And I inject pv irradiance 0.0 via sim inject
    And I set pv plan forecast to 0.0 kW

  @isolated
  Scenario: EV allowed to absorb surplus near departure
    Given I DELETE the EV session
    And I POST an EV session with target_soc 0.90 and departure in 0.33 hours
    And the EV is plugged with SoC at 0.30 (below target)
    And the battery SoC is reset to 1.0
    And the plan state is initialized with net import 0.0 kW
    When I create a PV surplus to produce negative deviation of 2.0 kW
    And I wait for the EV setpoint to change from baseline
    Then the absorber can adjust the EV charging
    And the EV charge setpoint is more positive than baseline
    And the EV moves closer to soc_target

  @isolated
  Scenario: EV departure guard prevents reduction near departure
    Given I DELETE the EV session
    And I POST an EV session with target_soc 0.90 and departure in 0.33 hours
    And the EV is plugged with SoC at 0.30 (below target)
    And the battery SoC is reset to 0.50
    And the plan state is initialized with net import 0.0 kW
    And I wait for a fresh plan
    When I create a positive deviation of 3.0 kW via base load injection
    And I wait for the battery setpoint to change from baseline
    Then the absorber skips the EV asset
    And the battery setpoint moved negative by at least 1.0 kW
    And the EV charge setpoint is unchanged from baseline
