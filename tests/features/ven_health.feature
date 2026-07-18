Feature: VEN Health Check
  The VEN exposes a componentised health endpoint (WP-T1,
  docs/plans/ven-ui-transparency.md) instead of a plain "ok" string.

  Scenario: Health endpoint reports ok status with all components healthy
    When I GET the VEN "/health" endpoint
    Then the VEN health response status is "ok"
    And the VEN health response has components ven_process, vtn_connection, storage, planner
