Feature: Shiftable Load Lifecycle (Plan B)
  Verify the runtime lifecycle of shiftable loads: plan allocation,
  appearance in GET /sim, auto-completion, mid-run deletion, and
  duplicate rejection.

  Background:
    Given the VEN is running with profile "test"

  # ── AC#1: Shiftable load appears in plan allocations ────────────────────

  Scenario: Shiftable load appears in plan allocations after POST
    Given I POST a shiftable load for asset "wm-1" at 2.0 kW for 60 minutes within 6 hours
    When I poll the VEN /plan until asset "wm-1" has an allocation
    Then the polled plan has an allocation for asset "wm-1"

  # ── AC#2: Running load appears in GET /sim ──────────────────────────────
  # Window must force slot-0 scheduling: duration fills entire window
  # so the MILP has exactly one valid start slot (the current one).

  Scenario: Running shiftable load appears in GET /sim
    Given I POST a shiftable load for asset "wm-2" at 2.0 kW for 60 minutes within 1 hours
    When I poll the VEN /sim until asset "wm-2" appears
    Then the polled sim has asset "wm-2" with power_kw > 0

  # ── AC#3: Load auto-completes after duration ────────────────────────────
  # 1-minute load in 30-minute window ⇒ only slot 0 valid with 1800s steps.

  @slow
  Scenario: Shiftable load auto-completes and disappears from GET /sim
    Given I POST a shiftable load for asset "wm-3" at 2.0 kW for 1 minutes within 30 minutes
    And I poll the VEN /sim until asset "wm-3" appears
    When I poll the VEN /sim until asset "wm-3" disappears
    Then the polled sim does not have asset "wm-3"

  # ── AC#5: Delete mid-run removes from /sim ──────────────────────────────

  Scenario: Deleting a running shiftable load removes it from GET /sim
    Given I POST a shiftable load for asset "wm-4" at 2.0 kW for 60 minutes within 1 hours
    And I poll the VEN /sim until asset "wm-4" appears
    When I DELETE shiftable load with saved id
    And I poll the VEN /sim until asset "wm-4" disappears
    Then the polled sim does not have asset "wm-4"

  # ── Duplicate rejection ─────────────────────────────────────────────────

  Scenario: POST rejects duplicate asset_id with 409
    Given I POST a shiftable load for asset "wm-dup" at 2.0 kW for 60 minutes within 6 hours
    When I POST a shiftable load for asset "wm-dup" at 1.5 kW for 30 minutes within 4 hours
    Then the response status is 409
