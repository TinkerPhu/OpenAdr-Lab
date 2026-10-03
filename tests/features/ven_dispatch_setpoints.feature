Feature: Direct setpoints (WP3.4 — BL-06/BL-24)
  DISPATCH_SETPOINT events steer the net site power directly via the battery
  while their window is active (the plan keeps running underneath);
  CHARGE_STATE_SETPOINT events create an EvSession targeting the given SoC.

  Background:
    Given I have a VTN token as "bl-client"

  Scenario: DISPATCH_SETPOINT steers net site power to the commanded value
    Given I create an open program "dispatch-test" and save its ID
    And I create a capacity event of type "DISPATCH_SETPOINT" with 2.0 kW for the saved program lasting 10 minutes
    Then the VEN net site power reaches 2.0 kW within 60 seconds with tolerance 0.5
    When I delete the saved capacity event

  # R-100: a VTN CHARGE_STATE_SETPOINT no longer creates an EvSession. A grid command
  # about an asset's state of charge is an external constraint, not the driver's intent
  # about their own travel, and with the session queue the two competed for one
  # calendar - a VTN session could block the user from booking their own car. The
  # parsing and withdrawal code is preserved but uncalled; what is asserted here is the
  # disabling, so re-wiring it cannot pass unnoticed.
  Scenario: CHARGE_STATE_SETPOINT creates no EV charging session
    Given I create an open program "charge-state-test" and save its ID
    And I create a capacity event of type "CHARGE_STATE_SETPOINT" with 0.9 kW for the saved program lasting 120 minutes
    Then the EV session queue stays empty for 20 seconds
    When I delete the saved capacity event
