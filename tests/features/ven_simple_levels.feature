Feature: SIMPLE load-shed levels (WP3.2)
  SIMPLE events carry a level 0-3. Level 2 ("moderate") clamps the planned
  import cap to the baseline forecast, deferring all flexible consumption;
  deleting the event (level back to 0 / normal) releases the clamp. The test
  profile's baseline is 0.5 kW, so a capped slot shows import_cap_kw ≈ 0.5
  against a 25 kW contractual limit.

  Background:
    Given I have a VTN token as "bl-client"

  Scenario: SIMPLE level steps 0 -> 2 -> 0 and the plan follows
    Given I create an open program "simple-level-test" and save its ID
    And I create a SIMPLE event of level 2 for the saved program lasting 30 minutes
    When I wait for the VEN /plan to have at least one slot with import_cap_kw at most 0.6
    When I delete the saved SIMPLE event
    And I wait for the VEN /plan to have no slot with import_cap_kw below 1.0

  # R-86. A VTN that wants a fleet not to respond on one instant sets randomizeStart on the
  # event's period. Each VEN delays the declared start by its own stable offset within that
  # window (a hash of its name and the event id, so it is reproducible, not a dice roll); the
  # offset is announced to the operator. Reports keep their cadence and are not staggered.
  Scenario: Two VENs begin a randomized event at their own offsets
    Given I create an open program "simple-randomize-test" and save its ID
    And I create a SIMPLE event of level 1 for the saved program lasting 120 minutes with a randomizeStart of 60 minutes
    When both VENs report a SIMPLE window for the saved event
    Then the two VENs begin the saved event at different times within the randomizeStart window
    When I delete the saved SIMPLE event
