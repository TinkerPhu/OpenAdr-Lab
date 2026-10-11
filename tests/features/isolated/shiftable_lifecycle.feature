Feature: Shiftable Load Lifecycle — isolated scenarios
  # Scenarios that require a clean VEN state due to timing sensitivity on Node1.
  # These pass reliably in isolation but can hit poll_until timeouts when run at
  # the end of the full suite under resource contention.

  Background:
    Given the VEN is running with profile "test"

  # ── AC#2: Running load appears in GET /sim ──────────────────────────────
  # "can only start in the current slot": the window ends a minute before a start in the next
  # slot could finish, so cost can never move the load later (GB-35). Loads that start here
  # are kept short: a started load cannot be cancelled and would run on into later scenarios.

  @isolated
  Scenario: Running shiftable load appears in GET /sim
    Given I POST a shiftable load for asset "wm-2" at 2.0 kW for 5 minutes that can only start in the current slot
    When I poll the VEN /sim until asset "wm-2" has started
    Then the polled sim has asset "wm-2" with power_kw > 0

  # ── AC#3: Load auto-completes after duration ────────────────────────────

  @slow @isolated
  Scenario: Shiftable load auto-completes and disappears from GET /sim
    Given I POST a shiftable load for asset "wm-3" at 2.0 kW for 1 minutes that can only start in the current slot
    And I poll the VEN /sim until asset "wm-3" has started
    And the polled sim has asset "wm-3" with power_kw > 0
    When I poll the VEN /sim until asset "wm-3" disappears
    Then the polled sim does not have asset "wm-3"

  # ── AC#5: Delete mid-run removes from /sim ──────────────────────────────

  @isolated
  Scenario: Deleting a running shiftable load removes it from GET /sim
    Given I POST a shiftable load for asset "wm-4" at 2.0 kW for 60 minutes within 80 minutes
    And I poll the VEN /sim until asset "wm-4" appears
    When I DELETE shiftable load with saved id
    And I poll the VEN /sim until asset "wm-4" disappears
    Then the polled sim does not have asset "wm-4"

  # ── A started load is non-interruptible: cancelling it is refused ──────────
  # Once running, the load draws its fixed power until its duration elapses; removing the
  # asset would desync the request record from the power still being drawn. The asset itself
  # answers "cancellable" (`Asset::is_cancellable`); the route reports 409.

  @isolated
  Scenario: Cancelling a shiftable load that is already running is refused
    Given I POST a shiftable load for asset "wm-5" at 2.0 kW for 5 minutes that can only start in the current slot
    And I poll the VEN /sim until asset "wm-5" has started
    When I DELETE shiftable load with saved id
    Then the response status is 409
    And I poll the VEN /sim until asset "wm-5" appears
