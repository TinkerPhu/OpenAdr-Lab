Feature: Shiftable Load Lifecycle — isolated scenarios
  # Scenarios that require a clean VEN state due to timing sensitivity on Pi4.
  # These pass reliably in isolation but can hit poll_until timeouts when run at
  # the end of the full suite under resource contention.

  Background:
    Given the VEN is running with profile "test"

  # ── AC#2: Running load appears in GET /sim ──────────────────────────────
  # Window must exceed duration to survive planner delay (window=80m > duration=60m).
  # NOTE: the window does NOT guarantee a single valid start slot — window offsets
  # are measured from the ALIGNED grid start (now truncated to the slot boundary),
  # so a mid-slot POST can make slot 1 valid too. The planner's earliest-start
  # tie-break (SHIFT_TIEBREAK_EUR_PER_SLOT) guarantees the load still starts in
  # the current slot when both are cost-equal, which is what these polls rely on.

  @isolated
  Scenario: Running shiftable load appears in GET /sim
    Given I POST a shiftable load for asset "wm-2" at 2.0 kW for 60 minutes within 80 minutes
    When I poll the VEN /sim until asset "wm-2" appears
    Then the polled sim has asset "wm-2" with power_kw > 0

  # ── AC#3: Load auto-completes after duration ────────────────────────────
  # 1-minute load in 30-minute window. Slot 1 can also be valid when the POST
  # lands mid-slot (aligned-now offset, see note above); the earliest-start
  # tie-break keeps the start in slot 0.
  #
  # Timing note: appearance in /sim takes ~125–150s on Pi4 (plan cycle → MILP
  # solve → adopt → dispatch tick), then a 1-min run + auto-complete detection.
  # The poll_until timeouts were raised (appears 240s, disappears 150s) so this
  # is no longer marginal against the inherent Pi4 latency. Not a code bug.

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
