Feature: Reactive correction notifications (BL-37)
  A sustained Layer-1 reactive correction (`controller::arbiter::reconcile`,
  gated by `deviation_arbiter_enabled`) is now visible on every tab via the
  existing Notifier (ring + SSE + persistence), not just while the Planner
  tab happens to be mounted. This exercises the edge-triggered producer
  end-to-end: enabling the arbiter, forcing a sustained deviation, and
  observing the start and clear notifications land in GET /notifications.
  #
  # Isolated (GB-35): the clear edge needs the arbiter to see the deviation
  # fall back under DEAD_BAND_KW (0.1 kW) for a tick, so it is bounded by sim
  # tick latency rather than by the VEN's own logic. Measured 2026-09-17 on
  # Node2 under the standing 17-VEN fleet: the scenario passes at 1-min load
  # 5.6-8.1 and times out at 10.1, on main and on a branch alike. A fresh VEN
  # state and the isolated pass's load-settle gate remove that coupling.

  Background:
    Given the VEN is running with profile "test"
    And the VEN-1 sim overrides are reset

  @isolated
  Scenario: A sustained deviation while the arbiter is enabled produces a start and a clear notification
    Given the deviation arbiter is enabled
    When I inject base_load_kw 3.0 with alpha 1.0 via sim inject
    And I wait for a user notification containing "Reactive correction active"
    And I clear the base_load_kw inject
    And I wait for a user notification containing "Reactive correction cleared"
    And the deviation arbiter is disabled
