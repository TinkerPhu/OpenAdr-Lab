Feature: VEN Dispatcher — Stage 4 (Plan Execution + Asset Ledger)
  Verify that the Dispatcher executes plan slot allocations: the EV sim
  charges when planned, EV sessions trigger replanning, and the asset ledger
  accumulates energy over time.
  Verify that the two-layer reactive control (Layer 1: battery correction,
  Layer 2: DeviceDeviation replan) is active and functional.

  Background:
    Given the VEN is running with profile "test"

  # --- EV session drives plan allocation ---

  Scenario: EV session drives dispatcher to allocate power to EV
    Given I inject ev_soc 0.50 via sim inject
    And I POST an EV session with target_soc 0.90 and departure in 12.0 hours
    When I wait for the VEN /plan to have an EV allocation in slots
    Then at least one firm slot has an allocation for asset "ev"

  # --- EV session CRUD via /ev-session ---

  Scenario: POST /ev-session creates a new EvSession
    When I POST an EV session with target_soc 0.95 and departure in 12.0 hours
    Then the response status is 201
    And the response JSON has field "id"
    And the response JSON has field "target_soc"

  Scenario: EvSession appears in GET /ev-session after POST
    When I POST an EV session with target_soc 0.95 and departure in 12.0 hours
    And I GET the EV session from /ev-session
    Then the response status is 200
    And the response JSON has field "id"

  # --- Asset energy ledger ---

  Scenario: GET /ledger returns per-asset energy accumulation after charging
    Given I inject ev_soc 0.5 via sim inject
    And I POST an EV session with target_soc 0.90 and departure in 12.0 hours
    When I wait for the VEN /plan to have an EV allocation in slots
    And I poll VEN /ledger until field "ev.energy_kwh" is greater than 0.0
    Then the response JSON has field "ev"
    And the response JSON field "ev.energy_kwh" is greater than 0.0

  # --- Layer 1: reactive battery correction ---
  # Verifies that apply_battery_correction_overlay reacts within one tick when
  # the grid deviates beyond the threshold, and holds the corrected setpoint.

  Scenario: Layer 1 corrects grid deviation immediately using battery
    Given the battery SoC is reset to 0.5
    When I wait for the VEN /plan endpoint to return a plan
    And I inject base_load_kw 10.0 with alpha 0.0 via sim inject
    Then within 5 seconds the VEN sim battery power_kw is less than -1.0

  # --- Layer 2: DeviceDeviation replan ---
  # Verifies that a sustained grid deviation (> threshold for deviation_trigger_ticks
  # consecutive ticks) fires a PlanTrigger::DeviceDeviation, causing a MILP replan.
  # The test profile sets deviation_trigger_ticks=30 (fires within the 60s timeout)
  # and base_load_alpha=0.0 keeps the load high between ticks.
  #
  # With the deviation absorber active, Tier 2 only fires when the absorber is
  # EXHAUSTED. We exhaust all absorber assets first so the full 15 kW extra load
  # appears as residual (> dead_band) and escalates to Tier 2.

  Scenario: Layer 2 triggers a DeviceDeviation replan after sustained grid deviation
    When I wait for the VEN /plan endpoint to return a plan
    And the battery SoC is reset to 0.10
    And I inject base_load_kw 15.0 with alpha 0.0 via sim inject
    And I poll VEN trace until a PlanCycle with trigger "DeviceDeviation" appears
    Then a PlanCycle with trigger "DeviceDeviation" was found in the trace
