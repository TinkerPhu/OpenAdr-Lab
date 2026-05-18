Feature: VEN Dispatcher — isolated scenarios
  # Layer 2: DeviceDeviation replan
  # Verifies that a sustained grid deviation (> threshold for deviation_trigger_ticks
  # consecutive ticks) fires a PlanTrigger::DeviceDeviation, causing a MILP replan.
  # With the deviation absorber active, Tier 2 only fires when the absorber is
  # EXHAUSTED. We exhaust all absorber assets first so the full 15 kW extra load
  # appears as residual (> dead_band) and escalates to Tier 2.
  # Timing note: polls VEN trace for DeviceDeviation PlanCycle (~39s in isolation).
  # Can hit poll_until timeout at suite end on Pi4 under resource contention.

  Background:
    Given the VEN is running with profile "test"
    And I set pv plan forecast to 0.0 kW

  @isolated
  Scenario: Layer 2 triggers a DeviceDeviation replan after sustained grid deviation
    When I wait for the VEN /plan endpoint to return a plan
    And the battery SoC is reset to 0.10
    And I inject base_load_kw 15.0 with alpha 0.0 via sim inject
    And I poll VEN trace until a PlanCycle with trigger "DeviceDeviation" appears
    Then a PlanCycle with trigger "DeviceDeviation" was found in the trace
