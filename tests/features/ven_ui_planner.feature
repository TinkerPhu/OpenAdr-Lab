@ven-ui
Feature: Planner Visualization Page

  The Planner page gives engineers and operators full transparency into the HEMS planner:
  why each asset was scheduled, what caused replans, packet progress, and plan health.

  Background:
    Given the VEN UI is open

  # ── Navigation ──────────────────────────────────────────────────────────────

  Scenario: Planner tab appears in navigation
    Then I see a nav button with testid "nav-planner"

  Scenario: Navigate to Planner page
    When I click the nav button "nav-planner"
    Then I see an element with testid "planner-heading"

  # ── Plan Header (US4) ────────────────────────────────────────────────────────

  Scenario: Plan header section is visible on Planner page
    When I navigate to the Planner page
    Then I see an element with testid "plan-header"

  Scenario: Plan header shows trigger badge and summary values
    When I navigate to the Planner page
    Then I see an element with testid "plan-trigger-badge"
    And I see an element with testid "plan-age"
    And I see an element with testid "plan-cost"
    And I see an element with testid "plan-import-kwh"
    And I see an element with testid "plan-co2"

  Scenario: Plan header shows no-plan state when planner has not run
    Given the planner has not yet generated a plan
    When I navigate to the Planner page
    Then I see an element with testid "plan-no-plan"

  # ── Trigger Timeline (US3) ───────────────────────────────────────────────────

  Scenario: Trigger timeline section is visible on Planner page
    When I navigate to the Planner page
    Then I see an element with testid "trigger-timeline"

  Scenario: Trigger timeline shows at least one event chip
    When I navigate to the Planner page
    Then at least one trigger chip is visible

  Scenario: Clicking a trigger chip shows detail popover
    When I navigate to the Planner page
    And I click the first trigger chip
    Then I see an element with testid "trigger-popover"

  # ── Decision Matrix (US1) ────────────────────────────────────────────────────

  Scenario: Decision matrix section is visible on Planner page
    When I navigate to the Planner page
    Then I see an element with testid "decision-matrix"

  Scenario: Decision matrix renders asset rows and tariff header
    When I navigate to the Planner page
    Then the decision matrix shows at least one asset row
    And the decision matrix shows the tariff header row

  Scenario: Decision matrix shows FIRM/FLEX boundary divider
    When I navigate to the Planner page
    And I click the element with testid "matrix-expand-horizon-btn"
    Then I see an element with testid "matrix-firm-flex-divider"

  Scenario: Clicking a matrix cell opens the step detail drawer
    When I navigate to the Planner page
    And I click the first visible matrix cell
    Then I see an element with testid "matrix-drawer"
    And I see an element with testid "matrix-drawer-reason"

  Scenario: Decision matrix collapses and expands
    When I navigate to the Planner page
    And I click the element with testid "matrix-collapse-btn"
    Then the decision matrix cells are hidden
    When I click the element with testid "matrix-collapse-btn"
    Then the decision matrix cells are visible

  Scenario: Decision matrix shows empty state when no plan available
    Given the planner has not yet generated a plan
    When I navigate to the Planner page
    Then I see an element with testid "matrix-empty"

  # ── Packet Board (US2) ───────────────────────────────────────────────────────

  Scenario: Packet board section is visible on Planner page
    When I navigate to the Planner page
    Then I see an element with testid "packet-board"

  Scenario: Packet board shows empty state when no packets exist
    Given no energy packets exist for this VEN
    When I navigate to the Planner page
    Then I see an element with testid "packet-board-empty"

  Scenario: Packet board shows packet groups
    Given at least one energy packet exists for this VEN
    When I navigate to the Planner page
    Then I see an element with testid "packet-group-active"
    And I see an element with testid "packet-group-queued"
    And I see an element with testid "packet-group-done"
