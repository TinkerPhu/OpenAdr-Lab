Feature: Asset Interface — forecast(timespan)
  Each asset type returns a self-describing TimeSeries via GET /forecast/:asset_id.
  Stubs return empty series; only after per-asset implementation do these scenarios pass.

  Background:
    Given the VEN is running with profile "test"

  # ── PV ──────────────────────────────────────────────────────────────────────

  Scenario: PV forecast during daytime returns non-empty power series
    When I GET /forecast/pv?timespan_s=3600 from the VEN
    Then the response status is 200
    And the forecast response has a non-empty samples list
    And the forecast interpolation is "linear"

  Scenario: PV forecast boundary point is present at end of timespan
    When I GET /forecast/pv?timespan_s=3600 from the VEN
    Then the response status is 200
    And the forecast samples are in ascending timestamp order
    And the last forecast sample is within 5 seconds of now plus 3600 seconds

  # ── Battery ─────────────────────────────────────────────────────────────────

  Scenario: Battery forecast returns power series with linear interpolation
    When I GET /forecast/battery?timespan_s=3600 from the VEN
    Then the response status is 200
    And the forecast response has a non-empty samples list
    And the forecast interpolation is "linear"

  # ── EV Charger ──────────────────────────────────────────────────────────────

  Scenario: EV charger forecast returns step-interpolated power series
    When I GET /forecast/ev?timespan_s=3600 from the VEN
    Then the response status is 200
    And the forecast response has a non-empty samples list
    And the forecast interpolation is "step"


  # ── Base Load ────────────────────────────────────────────────────────────────

  Scenario: Base load forecast returns constant step-interpolated power series
    When I GET /forecast/base_load?timespan_s=3600 from the VEN
    Then the response status is 200
    And the forecast response has a non-empty samples list
    And the forecast interpolation is "step"
    And all forecast sample values are equal

  # ── Heater ──────────────────────────────────────────────────────────────────

  Scenario: Heater forecast returns linear-interpolated power series
    When I GET /forecast/heater?timespan_s=3600 from the VEN
    Then the response status is 200
    And the forecast response has a non-empty samples list
    And the forecast interpolation is "linear"

  # ── Edge cases ───────────────────────────────────────────────────────────────

  Scenario: Zero timespan returns empty series
    When I GET /forecast/pv?timespan_s=0 from the VEN
    Then the response status is 200
    And the forecast samples list is empty
