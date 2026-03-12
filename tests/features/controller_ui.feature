Feature: VEN Controller Dashboard UI

  Background:
    Given I have a VTN token as "any-business"

  @ven-ui
  Scenario: Controller page renders without crashing when no price events are active
    Given no price events are active on VEN-1
    When I open the VEN-1 controller UI
    Then the controller rate chart empty state is visible
    And the controller packets table section is visible

  @ven-ui
  Scenario: Controller page renders plan card without crashing
    Given the VEN-1 controller has produced at least one plan
    When I open the VEN-1 controller UI
    Then the controller packets table section is visible
    And the controller plan card does not show an error

  @ven-ui
  Scenario: Controller rate chart renders when a PRICE event is active
    Given a PRICE event with import rate 0.25 EUR/kWh is active on VEN-1
    And I wait for VEN-1 to have rate data
    When I open the VEN-1 controller UI
    Then the controller rate chart with data is visible
