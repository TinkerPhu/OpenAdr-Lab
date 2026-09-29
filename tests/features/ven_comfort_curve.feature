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

  Scenario: A curve whose bid rises with fill is rejected
    # `ev-comfort-piecewise-core`: the bid is the most the user will pay for the
    # *next* kWh, so it cannot grow as the battery fills. Refusing it at the API
    # is what keeps the valuation concave and solvable with continuous variables.
    When I try to set a comfort curve for asset "ev" with points "0.2:0.10,0.9:0.40"
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
    Given I inject ev_soc 0.20 via sim inject
    When I set a comfort curve for asset "ev" with points "0.0:2.0,1.0:2.0"
    And I POST a soft-deadline user request for asset "ev" with target_soc 0.90 and latest_end in 6 hours
    And I wait for the VEN plan to be recomputed after the comfort-curve session
    Then the comfort-curve-driven plan has "ev" charging
    When I DELETE the comfort-curve-driven user request
    And I delete the comfort curve override for asset "ev"

  @use_case
  Scenario: A bid that covers only part of the energy buys that part (GB-41)
    # The user-observable statement of `ev-comfort-piecewise-core`. The curve bids
    # far above any tariff up to 50 % state of charge and nothing above it, while
    # the request asks for 90 %. The old all-or-nothing core priced the whole
    # block at one point of the curve and took zero when that did not clear —
    # four fleet VENs charging nothing for 24 h. Now the covered kWh are bought
    # and the rest are not, and nothing is reported as unmet, because a soft
    # request promised nothing.
    #
    # 60 kWh pack at 7 kW: 0.20 -> 0.50 SoC is 18 kWh, 0.20 -> 0.90 would be
    # 42 kWh and the 6 h window could deliver it. The band bounds are wide enough
    # to tolerate the live tariff and whatever the simulator does to SoC while the
    # plan is being recomputed, and narrow enough that neither "nothing" nor "all
    # of it" can pass.
    Given the comfort curve for asset "ev" reports source "default"
    And I set pv plan forecast to 0.0 kW
    And I inject ev_soc 0.20 via sim inject
    When I set a comfort curve for asset "ev" with points "0.0:5.0,0.5:5.0,0.55:0.0,1.0:0.0"
    And I POST a soft-deadline user request for asset "ev" with target_soc 0.90 and latest_end in 6 hours
    And I wait for the VEN plan to be recomputed after the comfort-curve session
    Then the comfort-curve-driven plan charges "ev" between 12.0 and 28.0 kWh
    And the comfort-curve-driven plan reports no unmet EV obligation
    When I DELETE the comfort-curve-driven user request
    And I delete the comfort curve override for asset "ev"

  @use_case
  Scenario: Comfort curve's CO2 bid changes whether the EV session commits to charging (BL-17)
    # Same mechanism as the price scenario above, but exercising the CO2 axis in
    # isolation: price bid stays 0.0 throughout so only the CO2 bid can move the
    # decision. The default planner weight (w_ghg) is deliberately tiny — real
    # gCO2/kWh-scale bids would round to nothing — so, exactly like the price
    # scenario's deliberately unrealistic EUR/kWh values, this uses a
    # correspondingly extreme gCO2/kWh bid to prove the axis is wired through
    # end-to-end, not to model a plausible real-world bid.
    Given the comfort curve for asset "ev" reports source "default"
    And I set pv plan forecast to 0.0 kW
    And I inject ev_soc 0.20 via sim inject
    When I set a comfort curve for asset "ev" with points "0.0:0.0:0,1.0:0.0:0"
    And I POST a soft-deadline user request for asset "ev" with target_soc 0.90 and latest_end in 6 hours
    And I wait for the VEN plan to be recomputed after the comfort-curve session
    Then the comfort-curve-driven plan has no "ev" charging
    When I DELETE the comfort-curve-driven user request
    Given I inject ev_soc 0.20 via sim inject
    When I set a comfort curve for asset "ev" with points "0.0:0.0:20000000,1.0:0.0:20000000"
    And I POST a soft-deadline user request for asset "ev" with target_soc 0.90 and latest_end in 6 hours
    And I wait for the VEN plan to be recomputed after the comfort-curve session
    Then the comfort-curve-driven plan has "ev" charging
    When I DELETE the comfort-curve-driven user request
    And I delete the comfort curve override for asset "ev"
