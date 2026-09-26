Feature: EV usage forecast fed to the planner (ev-usage-forecast)
  A profile-configured EV can hand its own predicted daily leave/return
  schedule to the planner as truthful per-slot availability, instead of only
  simulating it physically. The plan then knows, before the car has left, that
  the away window cannot be charged in — so it charges beforehand — and it
  needs no auto-created charge session to express that.

  Background:
    Given the VEN is running with profile "usage_forecast_test"

  Scenario: The forecast usage class is visible as read-only diagnostics
    When I GET /ev-usage-sim from the VEN
    Then the response status is 200
    And the response JSON field "mode" equals "forecast"
    And the response JSON field "engage_charge_planning" is true
    And the response JSON has field "next_trip.leave_at"
    And the response JSON has field "next_trip.return_at"

  Scenario: The plan charges in the home window and never during the predicted trip
    When I wait for the VEN /plan to have an EV allocation in slots
    Then the plan allocates no EV power while the EV is predicted away
    And the plan allocates EV power in the home window around the predicted trip

  Scenario: No charge session is invented to carry the predicted deadline
    When I GET /ev-session from the VEN
    Then the response status is 204
