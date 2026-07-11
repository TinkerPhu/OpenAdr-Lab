Feature: Direct setpoints (WP3.4 — BL-06/BL-24)
  DISPATCH_SETPOINT events steer the net site power directly via the battery
  while their window is active (the plan keeps running underneath);
  CHARGE_STATE_SETPOINT events create an EvSession targeting the given SoC.

  Background:
    Given I have a VTN token as "any-business"

  Scenario: DISPATCH_SETPOINT steers net site power to the commanded value
    Given I create an open program "dispatch-test" and save its ID
    And I create a capacity event of type "DISPATCH_SETPOINT" with 2.0 kW for the saved program lasting 10 minutes
    Then the VEN net site power reaches 2.0 kW within 60 seconds with tolerance 0.5
    When I delete the saved capacity event

  Scenario: CHARGE_STATE_SETPOINT creates an EvSession with the target SoC
    Given I create an open program "charge-state-test" and save its ID
    And I create a capacity event of type "CHARGE_STATE_SETPOINT" with 0.9 kW for the saved program lasting 120 minutes
    Then the VEN ev-session has target_soc 0.9 within 30 seconds
    When I delete the saved capacity event
