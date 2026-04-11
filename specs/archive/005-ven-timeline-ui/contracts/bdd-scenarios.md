# BDD Scenarios Contract: VEN Timeline UI

**Branch**: `005-ven-timeline-ui`
**Date**: 2026-03-16

New and updated BDD scenarios for `tests/features/`.

---

## New Feature File: `ven_timeline.feature`

```gherkin
Feature: VEN Asset Timeline Endpoints
  Background:
    Given the VEN is running and has been active for at least 60 seconds

  Scenario: GET /timeline/ev returns merged past and future points
    Given the EV has been charging for at least 1 tick
    And there is an active plan with at least one EV allocation
    When I call GET /timeline/ev?hours_back=1&hours_forward=1
    Then the response status is 200
    And the response JSON is a sorted array of timeline points
    And at least one point has ts before now
    And at least one point has ts after now
    And each point has a "values" object with key "power_kw"

  Scenario: GET /timeline/ev with hours_back=0 returns only future points
    When I call GET /timeline/ev?hours_back=0&hours_forward=1
    Then the response status is 200
    And all points have ts greater than or equal to now

  Scenario: GET /timeline/grid returns tariff and net power data
    Given at least one TariffSnapshot is available
    When I call GET /timeline/grid?hours_back=1&hours_forward=1
    Then the response status is 200
    And the response JSON is an array
    And at least one point has key "import_price_eur_kwh" in values

  Scenario: GET /timeline/all returns all configured assets and grid
    When I call GET /timeline/all?hours_back=1&hours_forward=1
    Then the response status is 200
    And the response JSON is an object
    And the object contains key "ev"
    And the object contains key "grid"
    And the object contains key "battery"

  Scenario: GET /timeline/{unknown} returns 404
    When I call GET /timeline/unknown_asset_xyz
    Then the response status is 404

  Scenario: Extended window returns correct horizon
    When I call GET /timeline/ev?hours_back=1&hours_forward=24
    Then the response status is 200
    And there are points up to 24 hours in the future

  Scenario: Future points carry cost and CO2 rates
    Given there is an active plan with tariff data
    When I call GET /timeline/ev?hours_back=0&hours_forward=1
    Then at least one future point has key "cost_rate_eur_h" in values
    And at least one future point has key "co2_rate_g_h" in values
```

---

## Updated Feature File: `ven_simulator.feature`

Assertions using `sim.assets.<id>.values.<key>` path:

```gherkin
  Scenario: GET /sim returns generic assets map
    When I call GET /sim
    Then the response JSON contains key "assets"
    And the assets object contains key "ev"
    And the ev entry has key "power_kw"
    And the ev entry has key "soc_pct"
```

(Existing named-field assertions updated to use `assets` path where applicable.)

---

## Updated Feature File: `controller/02_asset_cells.feature`

```gherkin
  Scenario: EV timeline chart shows past power data
    Given I open the VEN-1 controller V2 UI
    When the EV asset cell is visible
    Then the EV asset timeline chart is visible
    And the NOW reference line is visible on the EV timeline chart
    # (no gap assertion — chart now sourced from timeline endpoint)

  Scenario: Battery asset cell shows past power data
    Given I open the VEN-1 controller V2 UI
    When the battery asset cell is visible
    Then the battery asset timeline chart is visible

  Scenario: Per-cell extended window toggle expands EV horizon
    Given I open the VEN-1 controller V2 UI
    When I activate the extended window toggle on the EV cell
    Then the EV asset timeline chart shows a 24-hour forward window
```

---

## Existing Scenarios (unchanged)

- `controller/01_layout.feature` — no changes
- `controller/03_simulation_controls.feature` — updated to use dynamic schema-driven controls
- `controller/04_navigation.feature` — no changes
- `ven_uc_normal.feature` + UC-01–UC-12 scenarios — must remain passing with zero changes
