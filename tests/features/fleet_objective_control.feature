Feature: The fleet commands its VENs' objectives (fleet objective control)
  The VTN UI switches one VEN or the whole fleet between planning and the opportunistic
  objective through the BFF, which publishes a retained command on the VEN's
  `command/objective` leaf of the lab broker. The VEN applies it with its own objective setter,
  reports the result (and that the fleet set it) on its `objective` leaf, keeps it across a
  restart, and goes back to its profile objective when the command is released. Each VEN also
  reports its usage forecast to the VTN; the BFF sums the fleet's, and keeps the sum from before
  the last objective change.

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

  Scenario: The fleet forecast sums the VENs' forecasts and keeps the one from before a switch
    Given I have a VTN token as "bl-client"
    And I create an open program "fleet-forecast" and save its ID
    And I create a standing event for the saved program asking for a usage forecast
    When within 120 seconds the fleet forecast has slots
    And the fleet commands "ven-1" to run "opportunistic"
    Then within 150 seconds the fleet forecast has a before series

  @resilience
  Scenario: A restarted VEN applies the fleet's retained command again
    Given the fleet commands "ven-1" to run "opportunistic"
    And within 30 seconds VEN-1 reports "opportunistic" set by "fleet"
    When the "test-ven-1" service is restarted
    And I wait for the "test-ven-1" service to be healthy
    Then within 30 seconds VEN-1 reports "opportunistic" set by "fleet"
