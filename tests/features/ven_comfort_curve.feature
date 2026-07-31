Feature: User comfort-curve override (WP4.2, BL-19)
  A resident can replace an asset's built-in comfort/value curve with their
  own bid curve. The override survives until deleted; deleting restores the
  built-in default. Invalid curves are rejected with a reason.

  Scenario: Override is installed, reported, and reset to default
    Given the comfort curve for asset "ev" reports source "default"
    When I set a comfort curve for asset "ev" with points "0.5:0.40,0.9:0.25,1.0:0.10"
    Then the comfort curve for asset "ev" reports source "override"
    And the comfort curve for asset "ev" has 3 points
    When I delete the comfort curve override for asset "ev"
    Then the comfort curve for asset "ev" reports source "default"

  Scenario: Non-monotonic curve is rejected
    When I try to set a comfort curve for asset "ev" with points "0.9:0.40,0.5:0.25"
    Then the comfort curve request is rejected with status 422

  Scenario: Unknown asset returns 404
    When I try to set a comfort curve for asset "toaster" with points "0.5:0.40"
    Then the comfort curve request is rejected with status 404

  @use_case
  Scenario: Comfort curve changes whether the EV session commits to charging (BL-34)
    # The curve must reach the MILP solver, not just AssetRequestSlice storage —
    # a soft-deadline session (MayRun) only commits to its core target when the
    # curve's valuation beats the live import tariff. Use deliberately extreme
    # prices so the result holds regardless of today's actual tariff (the E2E
    # environment's tariff comes from a live VTN rate feed, not a fixed value).
    Given the comfort curve for asset "ev" reports source "default"
    And I set pv plan forecast to 0.0 kW
    And I inject ev_soc 0.20 via sim inject
    When I set a comfort curve for asset "ev" with points "0.0:0.0,1.0:0.0"
    And I POST a soft-deadline user request for asset "ev" with target_soc 0.90 and latest_end in 6 hours
    And I wait for the VEN plan to be recomputed after the comfort-curve session
    Then the comfort-curve-driven plan has no "ev" charging
    When I DELETE the comfort-curve-driven user request
    And I inject ev_soc 0.20 via sim inject
    And I set a comfort curve for asset "ev" with points "0.0:2.0,1.0:2.0"
    And I POST a soft-deadline user request for asset "ev" with target_soc 0.90 and latest_end in 6 hours
    And I wait for the VEN plan to be recomputed after the comfort-curve session
    Then the comfort-curve-driven plan has "ev" charging
    When I DELETE the comfort-curve-driven user request
    And I delete the comfort curve override for asset "ev"
