Feature: Grid-signal status aggregate (WP4.6)
  GET /signals is the one-round-trip source for the UI status strip:
  active alert / SIMPLE / dispatch windows plus the capacity state.

  Background:
    Given I have a VTN token as "any-business"

  Scenario: An alert window appears in /signals and clears on event deletion
    Given I create an open program "signals-test" and save its ID
    And I create an alert event "signals-alert-evt" of type "ALERT_GRID_EMERGENCY" for the saved program lasting 30 minutes
    When I wait for the VEN /signals to report an active alert
    Then the VEN /signals response has the keys "alerts, simple, dispatch, capacity"
    When I delete the saved alert event
    And I wait for the VEN /signals to report no active alert
