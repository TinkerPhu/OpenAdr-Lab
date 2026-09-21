Feature: Fleet telemetry (fleet-monitor phase 0)
  The VTN learns a VEN's state from reports, which describe intervals that have
  already closed. A fleet view needs the value now — so every VEN also
  publishes its live state to the lab broker, and the BFF holds the latest
  message per VEN.

  These two sources answer the same question and must not disagree about
  arithmetic: the fleet sum is over signed values, import positive and export
  negative, and a VEN that has said nothing is absent rather than zero.

  Scenario: The BFF reports whether the fleet feed is alive
    When I GET BFF health
    Then the BFF health shows the fleet feed connected

  # A VEN that has not reported contributes nothing to the sum and is not
  # counted as drawing zero: "we have not heard from it" and "it is drawing
  # nothing" are different facts, and a total that conflates them is wrong in
  # the way hardest to notice.
  Scenario: Fleet power sums only the VENs that have actually reported
    When I wait for the fleet feed to have at least 1 VEN
    Then every listed VEN either reports a power value or none at all
    And the fleet sum equals the sum of the reporting VENs

  # The live feed and the store answer the same question at two times. If the
  # store is silent while VENs are publishing, the history is quietly lying by
  # omission — which a dashboard cannot show and a reader cannot detect.
  Scenario: What the fleet published is still there a minute later
    When I wait for the fleet feed to have at least 1 VEN
    And I wait for the fleet history of the last 10 minutes to have samples
    Then the fleet history says which resolution it was drawn at
    And every history bucket counts the VENs it was summed from
