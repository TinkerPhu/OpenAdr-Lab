Feature: VEN Planner — Stage 3 (EnergyPacket + Algorithm)
  The VEN planner produces a Plan from RateSnapshots, EnergyPackets, and
  device sessions. The plan covers the configured planning horizon as a unified slot
  sequence.

  Background:
    Given the VEN is running with profile "test"
    And I set pv plan forecast to 0.0 kW

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
    And I POST an EV session with target_soc 0.90 and departure in 12.0 hours
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

  # --- Heater autonomous scheduling ---

  Scenario: Heater is scheduled autonomously when below comfort floor (no HeaterTarget needed)
    # When temperature is below temp_min, the planner sets MustRun mode and MUST
    # allocate heater power without requiring an explicit HeaterTarget session.
    Given I inject heater_temp_c 15.0 via sim inject
    When I wait for the VEN /plan to have a heater allocation in slots
    Then at least one firm slot has an allocation for asset "heater"

  Scenario: Plan has flexibility envelopes for far-horizon unscheduled energy
    Given I inject ev_soc 0.5 via sim inject
    And I POST an EV session with target_soc 0.90 and departure in 12.0 hours
    And I have a VTN token as "any-business"
    And I create a rate-system program and save its ID
    And I create a cheap 4-hour PRICE event for the saved program
    When I wait for the VEN /plan to have envelopes
    Then the plan.envelopes is a non-empty array

  # --- Peak-demand penalty threshold (WP6.3, BL-09) ---

  Scenario: Planner reschedules load to stay under a peak-demand penalty threshold
    Given the VEN is running with profile "penalty_test"
    And I inject ev_soc 0.5 via sim inject
    And I POST an EV session with target_soc 0.90 and departure in 12.0 hours
    When I wait for the VEN /plan to have an EV allocation in slots
    Then no plan slot's net_import_kw exceeds "10.0" within the horizon
    And at least one firm slot has an allocation for asset "ev"

  # --- PV forecast override (022-deterministic-test-env) ---

  @autoretry
  Scenario: PV forecast override does not trigger a replan
    # Setting pv_plan_kw must NOT send a PlanTrigger::AssetStateChange; the
    # planning loop must remain idle for at least 2 seconds after the inject.
    Given the system is idle
    When I set pv plan forecast to 0.0 kW
    Then no plan cycle is triggered within 2 seconds

  # --- Forward site-headroom forecast ---

  Scenario: Site headroom never claims PV export capability after dark
    # Guards the invariant plus the live wiring (dispatcher tick -> pre-lock
    # weather resolution -> per-slot forecast frames -> endpoint). The
    # multi-zone slot-alignment mechanism this was written for is covered
    # deterministically by the Rust test
    # `pv_ceiling_is_evaluated_at_each_slots_own_timestamp_on_a_zoned_horizon`
    # (fixed `now`, two zones) — the E2E profiles are single-zone, so this
    # scenario deliberately does not stand in for it.
    Given the VEN is running with profile "test"
    When I wait for the VEN site headroom forecast to cover the planning horizon
    Then no night slot in the site headroom forecast claims PV export capability
