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

  # W4: each EV has its own settings, at its own asset path.
  Scenario: An EV's opportunistic charging is switched off and on at its own settings
    When I turn opportunistic charging off for EV "ev"
    And I GET /assets/ev/ev-settings from the VEN
    Then the response status is 200
    And the response JSON field "opportunistic_charging_enabled" is false
    When I turn opportunistic charging on for EV "ev"
    And I GET /assets/ev/ev-settings from the VEN
    Then the response JSON field "opportunistic_charging_enabled" is true

  Scenario: An asset that is not an EV has no EV settings
    When I GET /assets/heater/ev-settings from the VEN
    Then the response status is 404
