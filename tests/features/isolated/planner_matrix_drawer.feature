Feature: Planner Visualization Page — isolated scenarios
  # GB-22 (docs/BACKLOG.md): un-isolated, this scenario races a real
  # backend poll_until (up to 300s, waiting for an EV plan allocation)
  # immediately followed by real Playwright browser navigation/click
  # during the main E2E pass — the same browser+backend-poll combo that
  # already caused features/isolated/controller_navigation.feature's
  # scenario to fail under host-load contention.

  Background:
    Given the VEN UI is open

  @isolated @ven-ui
  Scenario: Clicking a matrix cell with a step opens the step detail drawer
    Given I inject ev_soc 0.5 via sim inject
    And I POST an EV session with target_soc 0.90 and departure in 12.0 hours
    When I wait for the VEN /plan to have an EV allocation in slots
    And I navigate to the Planner page
    And I click the first matrix cell with nonzero power
    Then I see an element with testid "matrix-drawer"
