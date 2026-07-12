Feature: EV user-request modes (WP4.1, BL-28)
  The six UserRequestModes shape how the planner allocates EV charging:
  cost-blind (ASAP), free-energy-only (OPPORTUNISTIC / *_FREE), or
  budget-capped (MAX_COST — with a user notification when the budget
  cannot reach the target).

  Scenario: MAX_COST budget shortfall raises a user notification
    Given I POST a mode EV session with mode "MAX_COST" and budget 0.05
    When I wait for a user notification containing "budget"
    Then the mode EV session is deleted

  Scenario: ASAP_FREE plans no charging when no energy is free
    Given the PV plan forecast is pinned to 0 kW
    And I POST a mode EV session with mode "ASAP_FREE" and no budget
    When I wait for the VEN plan to be recomputed after the mode session
    Then the recomputed plan has no "ev" allocations
    And the mode EV session is deleted
    And the sim inject state is reset

  Scenario: BY_DEADLINE_FREE plans no charging when no energy is free
    Given the PV plan forecast is pinned to 0 kW
    And I POST a mode EV session with mode "BY_DEADLINE_FREE" and no budget
    When I wait for the VEN plan to be recomputed after the mode session
    Then the recomputed plan has no "ev" allocations
    And the mode EV session is deleted
    And the sim inject state is reset
