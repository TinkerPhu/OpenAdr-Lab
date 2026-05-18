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

  # ── AC#2, AC#3, AC#5: isolated scenarios ────────────────────────────────
  # Running/auto-complete/delete scenarios are in features/isolated/shiftable_lifecycle.feature.

  # ── Duplicate rejection ─────────────────────────────────────────────────

  Scenario: POST rejects duplicate asset_id with 409
    Given I POST a shiftable load for asset "wm-dup" at 2.0 kW for 60 minutes within 6 hours
    When I POST a shiftable load for asset "wm-dup" at 1.5 kW for 30 minutes within 4 hours
    Then the response status is 409
