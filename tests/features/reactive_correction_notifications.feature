Feature: Reactive correction notifications (BL-37)
  A sustained Layer-1 reactive correction (`controller::arbiter::reconcile`,
  gated by `deviation_arbiter_enabled`) is now visible on every tab via the
  existing Notifier (ring + SSE + persistence), not just while the Planner
  tab happens to be mounted. This exercises the edge-triggered producer
  end-to-end: enabling the arbiter, forcing a sustained deviation, and
  observing the start and clear notifications land in GET /notifications.

  Scenario: A sustained deviation while the arbiter is enabled produces a start and a clear notification
    Given the deviation arbiter is enabled
    When I inject base_load_kw 3.0 with alpha 1.0 via sim inject
    And I wait for a user notification containing "Reactive correction active"
    And I clear the base_load_kw inject
    And I wait for a user notification containing "Reactive correction cleared"
    And the deviation arbiter is disabled
