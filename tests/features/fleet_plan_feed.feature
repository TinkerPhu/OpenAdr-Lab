Feature: Fleet plan feed (what the fleet intends to import and export)
  The fleet's telemetry says what sites did. Their plans say what they intend, and a change of
  price shows up there within minutes, long before the hours it concerns have happened. Each VEN
  publishes the plan it adopts; the BFF keeps them and sums the plans that were current at an
  instant, so the same question can be asked of "now" and of a moment before a tariff was published.

  Background:
    Given I have a VTN token as "bl-client"

  Scenario: The plan a VEN is working to is on the fleet plan
    When I wait for the fleet plan to carry the plan the VEN is working to
    Then the fleet plan states import and export in kW, never negative
    And every grid point of the fleet plan says how many VENs it is built from

  # A new price makes the VEN replan. "Now" must follow it; an instant before the price was
  # published must still answer with the plan the fleet had then -- which is what lets a
  # before/after comparison be drawn from the stored plans alone.
  Scenario: A replan changes the fleet plan now but not as of an earlier instant
    Given I create a rate-system program and save its ID
    And I wait for the fleet plan to carry the plan the VEN is working to
    And I remember this instant and the plan the VEN has
    When I create a high-priced PRICE event for the saved program
    And I wait for the VEN to adopt a different plan
    And I wait for the fleet plan to carry the plan the VEN is working to
    Then the fleet plan as of the remembered instant still carries the earlier plan
    And the fleet plan now carries the later plan
