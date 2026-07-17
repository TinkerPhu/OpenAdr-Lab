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

  Scenario: Decision matrix collapse button is present
    When I navigate to the Planner page
    Then I see an element with testid "matrix-collapse-btn"

  Scenario: Clicking a matrix cell with a step opens the step detail drawer
    Given I inject ev_soc 0.5 via sim inject
    And I POST an EV session with target_soc 0.90 and departure in 12.0 hours
    When I wait for the VEN /plan to have an EV allocation in slots
    And I navigate to the Planner page
    And I click the first matrix cell with nonzero power
    Then I see an element with testid "matrix-drawer"

  Scenario: Decision matrix collapses and expands
    When I navigate to the Planner page
    And I click the element with testid "matrix-collapse-btn"
    Then the decision matrix cells are hidden
    When I click the element with testid "matrix-collapse-btn"
    Then the decision matrix cells are visible

  # ── Session Progress Board (SessionProgressBoard, replaced the Phase-D PacketProgressBoard) ──
  # Rendered on the Planner page (testids session-board / session-board-empty /
  # session-card-<id> / session-fill-<id> / session-deadline-<id>) and as a condensed
  # strip on the Dashboard (dash-session-strip, dash-objective-chip, session-chip-<id>).
  # Covered by UI unit tests (SessionProgressBoard.test.tsx); add browser scenarios here
  # when a BDD flow needs deadline-progress assertions.
