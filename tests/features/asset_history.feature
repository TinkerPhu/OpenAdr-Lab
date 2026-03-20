Feature: Asset Interface — past(timespan)
  Each asset returns historical power samples from the ring buffer via
  GET /timeline/:asset_id?hours_back=N.  The mandatory boundary point
  (first sample at exactly now − timespan) and QuantitySeries metadata are
  verified.  These scenarios fail when past() is not wired into the timeline
  handler and pass after T015–T021 are complete.

  Background:
    Given the VEN is running with profile "test"
    And the VEN has been running for at least 30 seconds

  # ── PV history ───────────────────────────────────────────────────────────────

  Scenario: PV 30-minute history returns samples
    When I GET /timeline/pv?hours_back=0.5 from the VEN
    Then the response status is 200
    And the timeline response has a non-empty samples list
    And the history quantity is "power"
    And the history unit is "kilowatt"
    And the history interpolation is "linear"

  Scenario: PV history boundary point is present at start of window
    When I GET /timeline/pv?hours_back=0.5 from the VEN
    Then the response status is 200
    And the history samples are in ascending timestamp order
    And the first history sample is within 1 second of now minus 1800 seconds

  # ── Battery history ──────────────────────────────────────────────────────────

  Scenario: Battery 30-minute history returns samples
    When I GET /timeline/battery?hours_back=0.5 from the VEN
    Then the response status is 200
    And the timeline response has a non-empty samples list
    And the history quantity is "power"
    And the history unit is "kilowatt"
    And the history interpolation is "linear"

  # ── Partial buffer ───────────────────────────────────────────────────────────

  Scenario: Requesting more history than available returns partial result
    When I GET /timeline/pv?hours_back=2.0 from the VEN
    Then the response status is 200
    And the timeline response is a valid history response

  # ── Edge cases ───────────────────────────────────────────────────────────────

  Scenario: No future-timestamped entries in history response
    When I GET /timeline/pv?hours_back=0.5 from the VEN
    Then the response status is 200
    And no history sample has a future timestamp
