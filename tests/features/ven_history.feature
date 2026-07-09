Feature: Persistent history routes (Phase 1, WP1.4)
  Read routes over the SQLite-backed HistoryPort store — distinct from the
  live in-memory /history/:asset_id ring-buffer endpoint.

  Background:
    Given the VEN is running with profile "test"

  @history
  Scenario: GET /history/grid returns a JSON array within the default range
    When I GET /history/grid from the VEN
    Then the response status is 200

  @history
  Scenario: GET /history/ticks returns a JSON array within the default range
    When I GET /history/ticks from the VEN
    Then the response status is 200

  @history
  Scenario: GET /history/ticks filtered by asset_id returns a JSON array
    When I GET /history/ticks?asset_id=ev from the VEN
    Then the response status is 200

  @history
  Scenario: GET /history/events returns a JSON array
    When I GET /history/events from the VEN
    Then the response status is 200

  @history
  Scenario: GET /history/reports returns a JSON array
    When I GET /history/reports from the VEN
    Then the response status is 200

  @history
  Scenario: GET /history/plans returns a JSON array
    When I GET /history/plans from the VEN
    Then the response status is 200

  @history
  Scenario: GET /history/ticks with an unparseable "from" is rejected
    When I GET /history/ticks?from=not-a-date from the VEN
    Then the response status is 400

  @history
  Scenario: GET /history/ticks with from after to is rejected
    When I GET /history/ticks?from=2026-01-02T00:00:00Z&to=2026-01-01T00:00:00Z from the VEN
    Then the response status is 400

  @history
  Scenario: GET /history/ticks spanning more than 7 days is rejected
    When I GET /history/ticks?from=2026-01-01T00:00:00Z&to=2026-01-10T00:00:00Z from the VEN
    Then the response status is 400

  @history
  Scenario: The live in-memory history route still resolves for a real asset id
    When I GET /history/ev from the VEN
    Then the response status is 200

  @history
  Scenario: GET /ledger without asset_id keeps its existing shape
    When I GET /ledger from the VEN
    Then the response status is 200

  @history
  Scenario: GET /ledger with asset_id returns current and closed_periods
    When I GET /ledger?asset_id=ev from the VEN
    Then the response status is 200
    And the response JSON has field "current"
    And the response JSON has field "closed_periods"

  @ven-ui
  Scenario: The History UI page opens via the nav bar
    Given I open the VEN-1 History UI
