Feature: VEN Planner — Stage 3 (EnergyPacket + Algorithm)
  The VEN planner produces a Plan from RateSnapshots, EnergyPackets, and
  device sessions. The plan covers a 24-hour horizon as a unified slot
  sequence.

  Background:
    Given the VEN is running with profile "test"

  # --- EV session drives planning ---

  Scenario: EV session appears in /ev-session after POST
    When I POST an EV session with target_soc 0.90 and departure in 12.0 hours
    And I GET the EV session from /ev-session
    Then the response status is 200
    And the response JSON has field "id"
    And the response JSON has field "target_soc"

  # --- Plan smoke test ---

  Scenario: GET /plan returns a non-null plan after VEN starts
    When I wait for the VEN /plan endpoint to return a plan
    Then the plan has field "id"
    And the plan has field "slots"
    And the plan has field "envelopes"

  # --- Slots ---

  Scenario: Plan slots cover the planning horizon
    When I wait for the VEN /plan endpoint to return a plan
    Then the plan.slots is a non-empty array

  # --- Allocation ---

  Scenario: Plan allocates EV to slots given a cheap PRICE event
    Given I inject ev_soc 0.5 via sim inject
    And I have a VTN token as "any-business"
    And I create a rate-system program and save its ID
    And I create a cheap 4-hour PRICE event for the saved program
    When I wait for the VEN /plan to have an EV allocation in slots
    Then at least one firm slot has an allocation for asset "ev"

  # --- EV session allocation ---

  Scenario: EV session drives the planner to allocate EV power
    Given I inject ev_soc 0.5 via sim inject
    And I POST an EV session with target_soc 0.90 and departure in 12.0 hours
    When I wait for the VEN /plan to have an EV allocation in slots
    Then at least one firm slot has an allocation for asset "ev"

  # --- Flexible envelopes ---

  Scenario: Plan has flexibility envelopes for far-horizon unscheduled energy
    Given I inject ev_soc 0.5 via sim inject
    And I have a VTN token as "any-business"
    And I create a rate-system program and save its ID
    And I create a cheap 4-hour PRICE event for the saved program
    When I wait for the VEN /plan to have envelopes
    Then the plan.envelopes is a non-empty array
