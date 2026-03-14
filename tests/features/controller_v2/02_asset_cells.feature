Feature: Controller V2 — Asset Cell Content

  Background:
    Given I have a VTN token as "any-business"
    And I open the VEN-1 controller V2 UI

  @ven-ui
  Scenario: Asset cell left section shows power, cost rate, and CO2eq rate
    Then the EV asset cell shows a power value
    And the EV asset cell shows a cost rate value
    And the EV asset cell shows a CO2eq rate value

  @ven-ui
  Scenario: Asset cell mid section shows a NOW reference line
    Then the EV asset timeline chart is visible
    And the NOW reference line is visible on the EV timeline chart

  @ven-ui
  Scenario: Battery asset cell shows State of Charge
    Then the battery asset cell shows a SoC value
