Feature: Shiftable Load Lifecycle — isolated scenarios
  # Scenarios that require a clean VEN state due to timing sensitivity on Pi4.
  # These pass reliably in isolation but can hit poll_until timeouts when run at
  # the end of the full suite under resource contention.

  Background:
    Given the VEN is running with profile "test"

  # ── AC#2: Running load appears in GET /sim ──────────────────────────────
  # Window must exceed duration to survive planner delay (window=80m > duration=60m),
  # but stay below duration + plan_step_s (60m + 30m = 90m) so MILP has only slot 0.

  @isolated
  Scenario: Running shiftable load appears in GET /sim
    Given I POST a shiftable load for asset "wm-2" at 2.0 kW for 60 minutes within 80 minutes
    When I poll the VEN /sim until asset "wm-2" appears
    Then the polled sim has asset "wm-2" with power_kw > 0

  # ── AC#3: Load auto-completes after duration ────────────────────────────
  # 1-minute load in 30-minute window ⇒ only slot 0 valid with 1800s steps.
  #
  # Timing note: this scenario takes ~145s on Pi4 (plan cycle + 1-min dispatch +
  # auto-complete detection). It passes reliably in isolation but can hit the
  # poll_until timeout when run at the end of the full suite under Pi4 resource
  # contention (prior scenarios leave the VEN planner slower). Not a code bug.

  @slow @isolated
  Scenario: Shiftable load auto-completes and disappears from GET /sim
    Given I POST a shiftable load for asset "wm-3" at 2.0 kW for 1 minutes within 30 minutes
    And I poll the VEN /sim until asset "wm-3" appears
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
