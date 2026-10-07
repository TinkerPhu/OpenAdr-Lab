Feature: Direct setpoints (WP3.4 — BL-06/BL-24)
  DISPATCH_SETPOINT events steer the net site power directly via the battery
  while their window is active (the plan keeps running underneath);
  CHARGE_STATE_SETPOINT events are received but not applied: the VEN says so.

  Background:
    Given I have a VTN token as "bl-client"

  Scenario: DISPATCH_SETPOINT steers net site power to the commanded value
    Given I create an open program "dispatch-test" and save its ID
    And I create a capacity event of type "DISPATCH_SETPOINT" with 2.0 kW for the saved program lasting 10 minutes
    Then the VEN net site power reaches 2.0 kW within 60 seconds with tolerance 0.5
    When I delete the saved capacity event

  # R-100: a VTN CHARGE_STATE_SETPOINT is not applied. This VEN decides when and how much a
  # household EV charges, so a grid command about its state of charge is neither the driver's
  # plan nor a planner input (docs/reference/WIRE_PROFILE.md). Not applying it silently would
  # be the failure `wire-contracts` forbids, so the VEN also announces it.
  Scenario: CHARGE_STATE_SETPOINT creates no EV session and is announced as not applied
    Given I create an open program "charge-state-test" and save its ID
    And I create a capacity event of type "CHARGE_STATE_SETPOINT" with 0.9 kW for the saved program lasting 120 minutes
    Then the VEN announces that "CHARGE_STATE_SETPOINT" in the saved capacity event is not applied
    And the EV session queue stays empty for 20 seconds
    When I delete the saved capacity event
