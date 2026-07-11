Feature: Capacity reservations constrain the planner (WP3.3, §8.10)
  IMPORT_CAPACITY_RESERVATION events grant a contracted import allowance.
  When the allowance is tighter than the contractual limit it binds the
  plan's per-slot import cap; deleting the event releases it.

  Background:
    Given I have a VTN token as "any-business"

  Scenario: A 3 kW import reservation binds the planned import cap
    Given I create an open program "reservation-test" and save its ID
    And I create a capacity event of type "IMPORT_CAPACITY_RESERVATION" with 3.0 kW for the saved program lasting 30 minutes
    When I wait for the VEN /plan to have at least one slot with import_cap_kw at most 3.0
    When I delete the saved capacity event
    And I wait for the VEN /plan to have no slot with import_cap_kw below 4.0
