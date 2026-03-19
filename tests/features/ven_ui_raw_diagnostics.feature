@ven-ui
Feature: VEN Raw Data Diagnostics Page

  Background:
    Given the VEN UI is open
    And the user navigates to the Raw Data page

  Scenario: Page renders three diagnostic cells
    Then I see the "Simulator State" cell
    And I see the "Tariffs" cell
    And I see the "Timeline" cell

  Scenario: Sim cell refreshes on button click
    When I click the refresh button in the "Simulator State" cell
    Then the Simulator State chart is displayed

  Scenario: Tariffs cell refreshes on button click
    When I click the refresh button in the "Tariffs" cell
    Then the Tariffs chart is displayed

  Scenario: Timeline cell shows series dropdown and refreshes
    When I select "grid" from the Timeline series dropdown
    And I click the refresh button in the "Timeline" cell
    Then the Timeline chart is displayed

  Scenario: Each cell refreshes independently
    When I click the refresh button in the "Simulator State" cell
    Then only the Simulator State cell shows a loading state or data
    And the Tariffs cell remains in its unloaded state

  Scenario: Timeline dropdown filters series
    When I click the refresh button in the "Timeline" cell
    Then the series dropdown lists the available asset series
    When I select "ev" from the Timeline series dropdown
    And I click the refresh button in the "Timeline" cell
    Then the Timeline chart displays data for "ev"
