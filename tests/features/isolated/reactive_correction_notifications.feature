Feature: Reactive correction notifications (BL-37)
  A sustained Layer-1 reactive correction (`controller::arbiter::reconcile`,
  gated by `deviation_arbiter_enabled`) is now visible on every tab via the
  existing Notifier (ring + SSE + persistence), not just while the Planner
  tab happens to be mounted. This exercises the edge-triggered producer
  end-to-end: enabling the arbiter, forcing a sustained deviation, and
  observing the start and clear notifications land in GET /notifications.
  #
  # R-88: a correction is released when its cause is gone — the deviation with
  # battery/EV back at plan falls inside DEAD_BAND_KW (0.1 kW) — and "cleared"
  # means exactly that release. So the clear edge follows the inject's removal
  # within a few ticks, and the scenario bounds it instead of waiting minutes.
  # The precondition makes sure nothing else in the site would hold the
  # correction (see `controller/arbiter/release.rs`).

  Background:
    Given the VEN is running with profile "test"
    And the VEN-1 sim overrides are reset

  @isolated
  Scenario: A sustained deviation while the arbiter is enabled produces a start and a clear notification
    Given the deviation arbiter is enabled
    And the arbiter has no deviation it would keep if released
    When I inject base_load_kw 3.0 with alpha 1.0 via sim inject
    And I wait for a user notification containing "Reactive correction active"
    And I clear the base_load_kw inject
    And I wait at most 60 seconds for a user notification containing "Reactive correction cleared"
    And the deviation arbiter is disabled
