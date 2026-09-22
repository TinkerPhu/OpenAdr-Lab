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

  # The chain §6.3 describes: the VTN publishes an event, the VEN says it saw
  # that exact version of it, and the BFF can answer "who saw this" afterwards.
  # Without the id on the trace entry this question has no answer at all —
  # event names are not unique, and an edited event keeps its name.
  Scenario: The fleet can be asked who saw a particular event
    Given I have a VTN token as "bl-client"
    And I create an open program "fleet-reaction-test" and save its ID
    And I create an IMPORT_CAPACITY_LIMIT event with limit 3.0 kW for the saved program
    When I wait for the fleet to report a reaction to the saved event
    Then each reacting VEN names the event version it saw

  # The operator-facing half: a number tells you the fleet drew 10 kW, a line
  # per VEN tells you which site moved when the limit landed. That is the
  # question the page exists to answer, and only a browser can check it.
  @ui
  Scenario: The fleet page draws a line for every VEN that is reporting
    Given I open the VTN UI
    When I navigate to the Fleet page
    Then the fleet chart has a line for every reporting VEN
    And hiding a VEN in the legend removes its line

  # §1: the power curves say what a site did; this says what it was told. A dip
  # at 09:15 means something different depending on whether a limit was in
  # force, and only the two together answer that.
  Scenario: The fleet's signals resolve to per-VEN bands
    Given I have a VTN token as "bl-client"
    And I create an open program "fleet-signal-test" and save its ID
    And I create an IMPORT_CAPACITY_LIMIT event with limit 2.5 kW for the saved program
    When I wait for the fleet signals of the last 10 minutes to include the saved event
    Then the signal band names its payload type and value
    And no event was left unread

