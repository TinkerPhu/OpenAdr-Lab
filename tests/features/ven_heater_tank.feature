Feature: Heater tank MILP trajectory model

  The tank energy trajectory model ensures the planner accounts for heat losses
  over the horizon, enforces the tank upper bound (no overheating), and applies
  a soft penalty when the tank drops below T_min.

  Background:
    Given the VEN is running with profile "test"

  # --- Upper bound: trajectory model prevents overheating via E_max constraint ---

  Scenario: Plan uses only mid-tier heater (not full-tier) near T_max
    # test profile: T_min=18°C, T_max=23°C, max_kw=3.0, mid_kw=1.5, thermal_mass=2.0 kWh/°C
    # Inject 22.5°C: E_init ≈ 9 kWh, E_max = 10 kWh, headroom ≈ 1 kWh.
    # Full-tier (3 kW × dt_h = 1.5 kWh/step net +0.975 kWh) would push E above E_max.
    # The trajectory upper bound E[t] ≤ E_max blocks full-tier scheduling near T_max,
    # so all heater slots must use mid-tier (1.5 kW) or be idle.
    Given I inject heater_temp_c 22.5 via sim inject
    When I wait for the VEN /plan to be recomputed after the sim inject
    Then the plan has no heater allocations at full power in the first 12 slots

  # --- Recovery: tank below T_min triggers heater scheduling ---

  Scenario: Plan schedules heater when tank is below T_min
    # Inject 17.5°C (below T_min=18): E_init = (17.5-18)×2 = -1 kWh.
    # s_low[0] = 1 kWh activates the soft violation penalty, driving heater scheduling.
    Given I inject heater_temp_c 17.5 via sim inject
    When I wait for the VEN /plan to have a heater allocation in slots
    Then at least one of the first 36 plan slots has a heater allocation

  # --- Tariff attraction: cheap PRICE event pulls heater into cheap window ---

  Scenario: Cheap PRICE event attracts heater into cheap tariff window
    # Tank at mid-comfort (20°C). A cheap 3-hour PRICE event starts now.
    # The tariff-aware planner should schedule heater power in cheap slots.
    Given I inject heater_temp_c 20.0 via sim inject
    And I have a VTN token as "any-business"
    And I create a rate-system program and save its ID
    And I create a cheap 3-hour PRICE event for the saved program
    When I wait for the VEN /plan to have a heater allocation in slots
    Then at least one of the first 36 plan slots has a heater allocation
