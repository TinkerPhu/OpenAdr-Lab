Feature: The fleet commands its VENs' objectives (fleet objective control)
  The VTN UI switches one VEN or the whole fleet between planning and the opportunistic
  objective through the BFF, which publishes a retained command on the VEN's
  `command/objective` leaf of the lab broker. The VEN applies it with its own objective setter,
  reports the result (and that the fleet set it) on its `objective` leaf, keeps it across a
  restart, and goes back to its profile objective when the command is released. A switch shows up
  in what the fleet intends: the BFF notes when each VEN's objective changed, the stored plans
  answer "as of" just before it (fleet_plan_feed.feature), and each VEN reports its usage forecast
  to the VTN at once.

  Scenario: The fleet switches one VEN and the VEN says the fleet set it
    When the fleet commands "ven-1" to run "opportunistic"
    Then within 30 seconds VEN-1 reports "opportunistic" set by "fleet"
    And within 30 seconds the BFF shows "ven-1" running "opportunistic" with nothing pending

  Scenario: Releasing the command returns the VEN to its profile objective
    Given the fleet commands "ven-1" to run "opportunistic"
    And within 30 seconds VEN-1 reports "opportunistic" set by "fleet"
    When the fleet releases "ven-1"
    Then within 30 seconds VEN-1 reports its profile objective set by "profile"

  Scenario: The whole fleet switches, one VEN at a time
    When the fleet switches every VEN to "opportunistic"
    Then the fleet switch goes to "ven-1" and "ven-2", spaced apart
    And within 30 seconds VEN-1 reports "opportunistic" set by "fleet"
    And within 45 seconds VEN-2 reports "opportunistic" set by "fleet"

  Scenario: An objective a VEN does not accept is refused, naming the VEN and the value
    When the fleet commands "ven-1" to run "max_happiness"
    Then the BFF refuses the command with 400 naming "ven-1" and "max_happiness"

  # The Fleet page draws the plan as it stood just before the last objective change behind the
  # current one, and opens each VEN's own plan from the same store.
  Scenario: The fleet keeps the plan from before a switch and serves the VEN's new plan
    Given within 30 seconds VEN-1 runs its profile objective and no fleet command
    And I wait for the fleet plan to carry the plan the VEN is working to
    When the fleet commands "ven-1" to run "opportunistic"
    Then within 30 seconds the BFF notes when the objective of "ven-1" changed
    And the fleet plan as of just before that change lists a plan of "ven-1" adopted before it
    And within 120 seconds the BFF serves the plan of "ven-1" as an "opportunistic" plan with a kW per asset
    And that plan is not the one the fleet plan listed from before the change

  # A USAGE_FORECAST asked for at frequency 0 (ad hoc) goes out when the plan's objective changes,
  # not only on the 15-minute cadence (openspec `opportunistic-objective` 6.4).
  Scenario: A VEN reports its usage forecast at once when the fleet switches its objective
    Given within 30 seconds VEN-1 runs its profile objective and no fleet command
    And I have a VTN token as "bl-client"
    And I create an open program "fleet-forecast" and save its ID
    And I create a standing event for the saved program asking for a usage forecast
    And within 120 seconds VEN-1 has reported a usage forecast for the event
    When the fleet commands "ven-1" to run "opportunistic"
    Then within 150 seconds VEN-1 reports a newer usage forecast for the event

  @resilience
  Scenario: A restarted VEN applies the fleet's retained command again
    Given the fleet commands "ven-1" to run "opportunistic"
    And within 30 seconds VEN-1 reports "opportunistic" set by "fleet"
    When the "test-ven-1" service is restarted
    And I wait for the "test-ven-1" service to be healthy
    Then within 30 seconds VEN-1 reports "opportunistic" set by "fleet"
