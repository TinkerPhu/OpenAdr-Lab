Feature: VEN Planner — Stage 3 (EnergyPacket + Algorithm)
  The VEN planner produces a Plan from RateSnapshots and EnergyPackets seeded
  from the device profile. The plan covers a 24-hour horizon as a unified
  slot sequence.

  Background:
    Given the VEN is running with profile "test"

  # --- Packet seeding ---

  Scenario: Profile-seeded EV packet appears in /packets
    When I GET /packets from the VEN
    Then the response status is 200
    And the response JSON is an array
    And the packets list has at least 1 item
    And at least one packet has asset_id "ev"

  # --- Plan smoke test ---

  Scenario: GET /plan returns a non-null plan after VEN starts
    When I wait for the VEN /plan endpoint to return a plan
    Then the plan has field "id"
    And the plan has field "slots"
    And the plan has field "packets"
    And the plan has field "envelopes"

  # --- Slots ---

  Scenario: Plan slots cover the planning horizon
    When I wait for the VEN /plan endpoint to return a plan
    Then the plan.firm_slots is a non-empty array

  # --- Allocation ---

  Scenario: Plan allocates EV to slots given a cheap PRICE event
    Given I inject ev_soc 0.5 via sim inject
    And I have a VTN token as "any-business"
    And I create a rate-system program and save its ID
    And I create a cheap 4-hour PRICE event for the saved program
    When I wait for the VEN /plan to have an EV allocation in firm_slots
    Then at least one firm slot has an allocation for asset "ev"

  # --- Packet status ---

  Scenario: EV packet is in Scheduled or Pending status after planning
    When I wait for the VEN /plan endpoint to return a plan
    Then the plan contains a packet with asset_id "ev" in a non-terminal status

  # --- Flexible envelopes ---

  Scenario: Plan has flexibility envelopes for far-horizon unscheduled energy
    Given I inject ev_soc 0.5 via sim inject
    And I have a VTN token as "any-business"
    And I create a rate-system program and save its ID
    And I create a cheap 4-hour PRICE event for the saved program
    When I wait for the VEN /plan to have envelopes
    Then the plan.envelopes is a non-empty array
