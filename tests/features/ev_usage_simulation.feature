Feature: EV usage simulation with plan-ahead (ev-usage-simulation)
  A profile-configured EV can simulate a daily leave/return usage pattern.
  With plan-ahead enabled and a guaranteed-daily trip, the planner is told
  about the EV's next leave in advance — via a simulated-origin charge
  session — before the car actually leaves, without any user or VTN request.

  Background:
    Given the VEN is running with profile "usage_sim_test"

  Scenario: Plan-ahead surfaces the next simulated trip as read-only diagnostics
    When I GET /ev-usage-sim from the VEN
    Then the response status is 200
    And the response JSON field "engage_charge_planning" is true
    And the response JSON has field "next_trip.leave_at"
    And the response JSON has field "next_trip.return_at"

  # ev-session-queue: plan-ahead now queues a rolling week of predicted trips, one
  # session each, rather than only the next leave. The queue is what changed; that each
  # session is SIMULATED_USAGE-origin and carries a departure is unchanged.
  Scenario: Plan-ahead queues a simulated charge session for each predicted trip
    When I poll the EV session queue until it has at least 1 session
    Then every queued EV session has origin "SIMULATED_USAGE"
    And the queued EV sessions do not overlap
