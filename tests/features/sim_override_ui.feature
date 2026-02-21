Feature: Simulation tab override switch UI

  Background:
    Given I have a VTN token as "any-business"
    And I create a program "sim-uc6-ui" targeting "ven-1-name" and save its ID
    And the VEN-1 sim overrides are reset

  @ven-ui
  Scenario: EV charge rate slider is disabled when CHARGE_STATE_SETPOINT event is active
    When I create a CHARGE_STATE_SETPOINT event "sim-uc6-charge" with value 7
    And I wait for VEN-1 reactor to show mode "CHARGE_SETPOINT"
    And I open the VEN-1 simulation UI
    Then the EV charge rate override toggle is shown
    And the EV charge rate slider is disabled
    And the EV charge rate caption contains "VTN commands"
    And the EV charge rate caption contains "7.0"

  @ven-ui
  Scenario: Owner override switch enables slider and shows VTN intent
    When I create a CHARGE_STATE_SETPOINT event "sim-uc6-override" with value 7
    And I wait for VEN-1 reactor to show mode "CHARGE_SETPOINT"
    And I open the VEN-1 simulation UI
    When I click the EV charge rate override toggle
    Then the EV charge rate slider is enabled
    And the EV charge rate caption contains "VTN intent"
    And the EV charge rate caption contains "7.0"

  @ven-ui
  Scenario: EV slider is enabled and labeled "idle default" when no event is active
    Given no CHARGE_STATE_SETPOINT events are active on VEN-1
    And I wait for VEN-1 reactor to show mode "IDLE"
    And I open the VEN-1 simulation UI
    Then the EV charge rate override toggle is not shown
    And the EV charge rate slider is enabled
    And the EV charge rate caption contains "No active event"
