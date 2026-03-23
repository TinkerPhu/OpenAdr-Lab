Feature: VEN Planner — PlanReason audit trail (Phase D CP3)
  The unified per-step loop emits a PlanReason with every PlanStep.
  These scenarios verify that the correct reason variants fire under
  the expected conditions.

  Background:
    Given the VEN is running with profile "test"

  # ── Scenario 1: Battery charges on cheap tariff ──────────────────────────
  Scenario: Battery charges when tariff is below median
    Given I have a VTN token as "any-business"
    And I create a rate-system program and save its ID
    And I create a 1-hour PRICE event at 0.05 EUR/kWh for the saved program
    When I wait for a "CHEAP_TARIFF" PlanStep for asset "battery"
    Then that PlanStep has setpoint_kw greater than 0.0

  # ── Scenario 2: Battery discharges on expensive tariff ───────────────────
  Scenario: Battery discharges when tariff is above median
    Given I have a VTN token as "any-business"
    And I create a rate-system program and save its ID
    And I create a 1-hour PRICE event at 0.40 EUR/kWh for the saved program
    When I wait for a "EXPENSIVE_TARIFF" PlanStep for asset "battery"
    Then that PlanStep has setpoint_kw less than 0.0

  # ── Scenario 3: EV charges under deadline pressure ───────────────────────
  Scenario: EV charges under deadline pressure
    Given I POST an EV packet with target_soc 0.8, desired_power_kw 1.0, and latest_end_h 1.0
    When I wait for the VEN /plan to have steps for asset "ev"
    Then at least one PlanStep for asset "ev" has reason kind "FIRM_OBLIGATION"
    And that PlanStep has setpoint_kw greater than 0.0

  # ── Scenario 4: Battery is idle with no active packets and median tariff ──
  Scenario: Battery is idle when no packets and tariff is at median
    When I wait for all PlanSteps for asset "battery" to have reason kind "IDLE"
    Then all PlanSteps for asset "battery" have reason kind "IDLE"

  # ── Scenario 5: GET /plan?summary omits the steps array ──────────────────
  Scenario: GET /plan?summary returns plan without steps
    When I wait for the VEN /plan endpoint to return a plan
    And I request the VEN plan summary
    Then the response status is 200
    And the response JSON has field "id"
    And the response body has an empty "steps" array
