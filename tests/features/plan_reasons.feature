Feature: VEN Planner — PlanReason audit trail (Phase D CP3)
  The unified per-step loop emits a PlanReason with every PlanStep.
  These scenarios verify that the correct reason variants fire under
  the expected conditions.

  Background:
    Given the VEN is running with profile "test"

  # ── Scenario 1: Battery charges from grid only when depletion is predicted ──
  # The planner must see a future expensive discharge period that would drain the
  # battery below min_soc. Only then does it schedule a grid charge at the
  # cheapest slot before the depletion. With soc=0.20 and baseline=0.5kW, the
  # 3h expensive period (0.40 EUR/kWh) drains ~1.5kWh leaving soc below min_soc.
  # The preceding cheap slot (0.05 EUR/kWh) is the cheapest option → CHEAP_TARIFF.
  Scenario: Battery charges from grid at cheap tariff before an expensive discharge period
    Given the VEN is running with profile "no_pv_test"
    And I have a VTN token as "any-business"
    And I create a rate-system program and save its ID
    And I create a cheap-then-expensive PRICE event at 0.05 then 0.40 EUR/kWh for the saved program
    And the battery SoC is reset to 0.20
    When I wait for a "CHEAP_TARIFF" PlanStep for asset "battery"
    Then that PlanStep has setpoint_kw greater than 0.0

  # ── Scenario 8: No grid charge when tariff is cheap but no expensive period ─
  # Without a future expensive discharge period, the shadow sim predicts no
  # depletion → charge_plan is empty → battery stays idle despite cheap tariff.
  Scenario: Battery stays idle at cheap tariff when no future expensive discharge period exists
    Given I have a VTN token as "any-business"
    And I create a rate-system program and save its ID
    And I create a 1-hour PRICE event at 0.05 EUR/kWh for the saved program
    And I inject pv irradiance 0.0 via sim inject
    And the battery SoC is reset to 0.50
    When I wait for all PlanSteps for asset "battery" to have reason kind "IDLE|SURPLUS_ABSORPTION|SOC_CEILING|SOC_FLOOR"
    Then all PlanSteps for asset "battery" have reason kind "IDLE|SURPLUS_ABSORPTION|SOC_CEILING|SOC_FLOOR"

  # ── Scenario 2: Battery discharges on expensive tariff ───────────────────
  Scenario: Battery discharges when tariff is above median
    Given I have a VTN token as "any-business"
    And I create a rate-system program and save its ID
    And I create a 1-hour PRICE event at 0.40 EUR/kWh for the saved program
    And I inject pv irradiance 0.0 via sim inject
    When I wait for a "EXPENSIVE_TARIFF" PlanStep for asset "battery"
    Then that PlanStep has setpoint_kw less than 0.0

  # ── Scenario 3: EV charges under deadline pressure ───────────────────────
  Scenario: EV charges under deadline pressure
    Given I inject ev_soc 0.5 via sim inject
    And I POST an EV packet with target_soc 0.8, desired_power_kw 1.0, and latest_end_h 1.0
    When I wait for the VEN /plan to have steps for asset "ev"
    Then at least one PlanStep for asset "ev" has reason kind "FIRM_OBLIGATION"
    And that PlanStep has setpoint_kw greater than 0.0

  # ── Scenario 4: Battery is idle with no active packets and median tariff ──
  Scenario: Battery is idle when no packets and tariff is at median
    Given I inject pv irradiance 0.0 via sim inject
    And the battery SoC is reset to 0.5
    When I wait for all PlanSteps for asset "battery" to have reason kind "IDLE|SURPLUS_ABSORPTION"
    Then all PlanSteps for asset "battery" have reason kind "IDLE|SURPLUS_ABSORPTION"

  # ── Scenario 6: Battery absorbs PV surplus regardless of tariff (Rule 8b) ──
  Scenario: Battery charges from PV surplus regardless of tariff
    Given I inject pv irradiance 1.0 via sim inject
    And the battery SoC is reset to 0.2
    When I wait for a "SURPLUS_ABSORPTION" PlanStep for asset "battery"
    Then that PlanStep has setpoint_kw greater than 0.0

  # ── Scenario 7: Battery does not discharge into PV surplus (Rule 10 guard) ─
  Scenario: Battery does not discharge when PV covers the site load
    Given I have a VTN token as "any-business"
    And I create a rate-system program and save its ID
    And I create a 1-hour PRICE event at 0.40 EUR/kWh for the saved program
    And I inject pv irradiance 1.0 via sim inject
    And the battery SoC is reset to 0.8
    When I wait for all PlanSteps for asset "battery" to have reason kind "SURPLUS_ABSORPTION|IDLE"
    Then all PlanSteps for asset "battery" have reason kind "SURPLUS_ABSORPTION|IDLE"

  # ── Scenario 5: GET /plan?summary omits the steps array ──────────────────
  Scenario: GET /plan?summary returns plan without steps
    When I wait for the VEN /plan endpoint to return a plan
    And I request the VEN plan summary
    Then the response status is 200
    And the response JSON has field "id"
    And the response body has an empty "steps" array
