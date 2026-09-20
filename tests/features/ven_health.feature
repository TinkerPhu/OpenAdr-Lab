Feature: VEN Health Check
  The VEN exposes a componentised health endpoint (WP-T1,
  docs/history/project_journal.md, search "WP-T") instead of a plain "ok" string.

  Scenario: Health endpoint reports ok status with all components healthy
    When I GET the VEN "/health" endpoint
    Then the VEN health response status is "ok"
    And the VEN health response has components ven_process, vtn_connection, storage, planner

  # A refused wire object that nobody can see is the same failure as one we
  # accepted silently, so the refusal has a surface of its own. The refusal
  # *path* is unit-tested (controller::wire_reject) rather than driven from
  # here: the VTN will not store an object it cannot parse itself, so a
  # malformed event cannot honestly be injected through it end to end.
  Scenario: Health exposes wire conformance and the live fleet is conformant
    When I GET the VEN "/health" endpoint
    Then the VEN health response has components wire_conformance
    And the VEN health component "wire_conformance" is "ok"
