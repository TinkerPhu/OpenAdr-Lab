Feature: Controller V2 — Navigation and Layout Controls

  Background:
    Given I have a VTN token as "any-business"
    And I open the VEN-1 controller V2 UI

  @ven-ui
  Scenario: Pin button is present on each cell
    Then the EV asset cell has a pin button
    And the grid tariff cell has a pin button

  @ven-ui
  Scenario: Pinning a cell moves it to the pinned zone
    When I click the pin button on the EV asset cell
    Then the EV asset cell is visible in the pinned zone

  @ven-ui
  Scenario: Unpinning a cell removes it from the pinned zone
    When I click the pin button on the EV asset cell
    And I click the pin button on the EV asset cell again
    Then the EV asset cell is not in the pinned zone

  @ven-ui
  Scenario: Collapse left section hides the left panel
    When I click the collapse left button on the EV asset cell
    Then the EV asset cell left section is not visible

  @ven-ui
  Scenario: Collapse right section hides the right panel
    When I click the collapse right button on the EV asset cell
    Then the EV asset cell right section is not visible
