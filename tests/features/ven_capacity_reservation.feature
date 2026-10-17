Feature: Capacity reservations constrain the planner (WP3.3, §8.10)
  IMPORT_CAPACITY_RESERVATION events grant a contracted import allowance.
  When the allowance is tighter than the contractual limit it binds the
  plan's per-slot import cap; deleting the event releases it. It reaches the
  planner as a capacity change, so it is planned for at once, never held like a
  price change (GB-55).

  Background:
    Given I have a VTN token as "bl-client"

  Scenario: A 3 kW import reservation binds the planned import cap
    Given I create an open program "reservation-test" and save its ID
    And I create a capacity event of type "IMPORT_CAPACITY_RESERVATION" with 3.0 kW for the saved program lasting 30 minutes
    When I wait for a capacity-change replan with at least one slot capped at 3 kW
    When I delete the saved capacity event
    And I wait for the VEN /plan to have no slot with import_cap_kw below 4.0

  Scenario: The reservation request shrinks while a reservation window is active
    Given I create an open program "reservation-request-test" and save its ID
    And I announce an import capacity subscription of 0.5 kW starting in 0 minutes for 4320 minutes
    And I note the VEN's import reservation request
    When I announce an import capacity reservation of 0.25 kW starting in 0 minutes for 30 minutes
    Then the VEN's import reservation request is 0.25 kW lower within 120 seconds
