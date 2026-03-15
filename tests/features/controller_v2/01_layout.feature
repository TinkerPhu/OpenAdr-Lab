Feature: Controller V2 — Page Layout

  Background:
    Given I have a VTN token as "any-business"
    And I open the VEN-1 controller V2 UI

  @ven-ui
  Scenario: Grid cells appear above asset cells by default
    Then the grid tariff cell is visible
    And the grid accumulated cell is visible
    And the grid cells appear above the asset cells

  @ven-ui
  Scenario: Page is scrollable and grid cells are not fixed by default
    Then the controller V2 scrollable content area is visible
    And the grid tariff cell is not sticky by default

  @ven-ui
  Scenario: At least one asset cell is present
    Then at least one asset cell is visible
