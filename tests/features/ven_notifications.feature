Feature: VEN notification history endpoint (030)
  The VEN serves its persisted notification history over
  GET /notifications/history with dedup-aware rows (count, last_seen_at)
  and an optional severity filter. The dedup collapse itself (repeated
  storage failures appear once with a count) is verified at the use-case
  layer (services::notify tests); end-to-end this feature verifies the
  HTTP contract of the viewer.

  Scenario: History endpoint serves dedup-aware notification rows
    When I GET the VEN "/notifications/history" endpoint
    Then the VEN notification history response is a list of dedup-aware rows

  Scenario: Severity filter returns only matching rows
    When I GET the VEN "/notifications/history?severity=ALERT" endpoint
    Then every VEN notification history row has severity "ALERT"

  Scenario: Invalid severity is rejected with a JSON error
    When I GET the VEN "/notifications/history?severity=BOGUS" endpoint
    Then the VEN notification history response is 400 with a JSON error mentioning "invalid severity"
