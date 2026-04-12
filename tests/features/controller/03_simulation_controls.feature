@phase-controller
Feature: Controller V2 — Simulation Controls

  Background:
    Given I have a VTN token as "any-business"
    And the VEN-1 sim overrides are reset
    And I open the VEN-1 controller V2 UI

  @ven-ui
  Scenario: EV plugged toggle is visible in right section
    Then the EV plugged toggle is visible in the EV cell right section

  @ven-ui
  Scenario: EV SoC slider is visible in the Status Settings accordion
    Then the EV SoC slider is visible in the EV cell right section

  @ven-ui
  Scenario: Toggling EV plugged switch triggers a POST to sim override
    When I toggle the EV plugged switch in the controller V2 EV cell
    Then the EV plugged state changes in VEN-1 sim override
