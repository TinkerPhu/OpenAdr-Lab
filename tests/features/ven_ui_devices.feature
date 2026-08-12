@ven-ui
Feature: Devices Page — Baseline Override

  BL-42: the backend `/baseline-override` capability (GET/POST/DELETE) had client
  and hook wiring but no rendering UI. This exercises the BaselineOverrideCard
  control added to the Devices page end to end against the live VEN UI.

  Background:
    Given the VEN UI is open
    And no baseline override is active

  Scenario: Baseline override card is visible on the Devices page
    When I navigate to the Devices page
    Then I see an element with testid "baseline-override-card"

  Scenario: No active override shows an empty state with Clear disabled
    When I navigate to the Devices page
    Then I see an element with testid "baseline-override-card"
    And the element with testid "baseline-clear-btn" is disabled

  Scenario: End-to-end baseline override set and clear via the UI
    When I navigate to the Devices page
    And I click the element with testid "baseline-add-btn"
    And I fill the datetime-local field "baseline-slot-start-0" with "2026-04-12T07:00"
    And I fill the field "baseline-add-kw-0" with "1.5"
    And I click the element with testid "baseline-save-btn"
    Then the field "baseline-add-kw-0" has value "1.5"
    When I click the element with testid "baseline-clear-btn"
    Then I see the text "No baseline override active"
