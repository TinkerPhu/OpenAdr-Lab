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
