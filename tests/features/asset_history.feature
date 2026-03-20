Feature: Asset Interface — history(timespan)
  Each asset returns historical power samples from the ring buffer via
  GET /history/:asset_id?timespan_s=N, returning a full QuantitySeries response
  (samples, quantity, unit, interpolation). These scenarios fail when history()
  stubs return empty and pass after T015–T021 are complete.

  Background:
    Given the VEN is running with profile "test"
    And the VEN has been running for at least 30 seconds

  # ── PV history ───────────────────────────────────────────────────────────────

  Scenario: PV 30-minute history returns samples
    When I GET /history/pv?timespan_s=1800 from the VEN
    Then the response status is 200
    And the history response has a non-empty samples list
    And the history quantity is "power"
    And the history unit is "kilowatt"
    And the history interpolation is "linear"

  Scenario: PV history boundary point is present at start of window
    When I GET /history/pv?timespan_s=1800 from the VEN
    Then the response status is 200
    And the history samples are in ascending timestamp order
    And the first history sample is within 1 second of now minus 1800 seconds

  # ── Battery history ──────────────────────────────────────────────────────────

  Scenario: Battery 30-minute history returns samples
    When I GET /history/battery?timespan_s=1800 from the VEN
    Then the response status is 200
    And the history response has a non-empty samples list
    And the history quantity is "power"
    And the history unit is "kilowatt"
    And the history interpolation is "linear"

  # ── Partial buffer ───────────────────────────────────────────────────────────

  Scenario: Requesting more history than available returns partial result without error
    When I GET /history/pv?timespan_s=7200 from the VEN
    Then the response status is 200
    And the history response is a valid history response

  # ── Edge cases ───────────────────────────────────────────────────────────────

  Scenario: No future-timestamped entries in history response
    When I GET /history/pv?timespan_s=1800 from the VEN
    Then the response status is 200
    And no history sample has a future timestamp
