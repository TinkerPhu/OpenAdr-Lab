Feature: SIMPLE load-shed levels (WP3.2)
  SIMPLE events carry a level 0-3. Level 2 ("moderate") clamps the planned
  import cap to the baseline forecast, deferring all flexible consumption;
  deleting the event (level back to 0 / normal) releases the clamp. The test
  profile's baseline is 0.5 kW, so a capped slot shows import_cap_kw ≈ 0.5
  against a 25 kW contractual limit.

  Background:
    Given I have a VTN token as "any-business"

  Scenario: SIMPLE level steps 0 -> 2 -> 0 and the plan follows
    Given I create an open program "simple-level-test" and save its ID
    And I create a SIMPLE event of level 2 for the saved program lasting 30 minutes
    When I wait for the VEN /plan to have at least one slot with import_cap_kw at most 0.6
    When I delete the saved SIMPLE event
    And I wait for the VEN /plan to have no slot with import_cap_kw below 1.0
