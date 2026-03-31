Feature: Uniform-Grid Timeline API (RF-05c)
  GET /timeline/all and GET /timeline/:asset_id return all assets on a shared
  uniform time grid with a now-point between history and future portions.

  Background:
    Given the VEN is running

  # ── US1: Grid-aligned multi-asset timeline ──────────────────────────────────

  Scenario: GET /timeline/all returns arrays of equal length for all assets
    When I GET /timeline/all?hours_back=1&hours_forward=1 from the VEN
    Then the response status is 200
    And the response JSON is an object
    And all asset arrays have the same length

  Scenario: All assets share the same ts at each index position
    When I GET /timeline/all?hours_back=1&hours_forward=1 from the VEN
    Then the response status is 200
    And the response JSON is an object
    And all asset arrays have identical ts values at each index

  Scenario: Grid-portion timestamps are uniformly spaced
    When I GET /timeline/all?resolution=10&hours_back=1&hours_forward=1 from the VEN
    Then the response status is 200
    And the response JSON is an object
    And the grid portions have uniform spacing of 10 seconds

  Scenario: Grid timestamps are snapped to round boundaries
    When I GET /timeline/all?resolution=10&hours_back=1&hours_forward=1 from the VEN
    Then the response status is 200
    And the response JSON is an object
    And all grid-portion timestamps are multiples of 10 seconds

  # ── US2: Now-point ──────────────────────────────────────────────────────────

  Scenario: Each asset array contains a now-point between history and future
    When I GET /timeline/all?resolution=30&hours_back=1&hours_forward=1 from the VEN
    Then the response status is 200
    And the response JSON is an object
    And each asset array has a now-point between history and future grid portions

  Scenario: The now-point ts is the same across all assets
    When I GET /timeline/all?resolution=30&hours_back=1&hours_forward=1 from the VEN
    Then the response status is 200
    And the response JSON is an object
    And the now-point ts is identical across all assets

  Scenario: Empty future buckets have values null
    When I GET /timeline/all?hours_back=0&hours_forward=6 from the VEN
    Then the response status is 200
    And the response JSON is an object
    And at least one future point has null values

  Scenario: Response format is unchanged
    When I GET /timeline/all from the VEN
    Then the response status is 200
    And the response JSON is an object
    And each value is an array of objects with ts and values fields

  # ── US3: Resolution parameter ──────────────────────────────────────────────

  Scenario: resolution=30 returns 30-second spacing
    When I GET /timeline/all?resolution=30&hours_back=1&hours_forward=1 from the VEN
    Then the response status is 200
    And the response JSON is an object
    And the grid portions have uniform spacing of 30 seconds

  Scenario: Default auto-resolution targets approximately 300 points
    When I GET /timeline/all?hours_back=1&hours_forward=1 from the VEN
    Then the response status is 200
    And the response JSON is an object
    And the total array length is between 200 and 500

  Scenario: max_points=150 produces equivalent resolution
    When I GET /timeline/all?max_points=150&hours_back=1&hours_forward=1 from the VEN
    Then the response status is 200
    And the response JSON is an object
    And the total array length is between 100 and 250

  Scenario: resolution takes precedence over max_points
    When I GET /timeline/all?resolution=60&max_points=10&hours_back=1&hours_forward=1 from the VEN
    Then the response status is 200
    And the response JSON is an object
    And the grid portions have uniform spacing of 60 seconds

  # ── US4: Single-asset endpoint ─────────────────────────────────────────────

  Scenario: GET /timeline/ev returns uniformly spaced ts with now-point
    When I GET /timeline/ev?resolution=30&hours_back=1&hours_forward=1 from the VEN
    Then the response status is 200
    And the response JSON is an array
    And the single-asset grid portions have uniform spacing of 30 seconds
    And the single-asset array has a now-point

  Scenario: GET /timeline/ev with resolution=30 returns 30-second spacing
    When I GET /timeline/ev?resolution=30&hours_back=1&hours_forward=1 from the VEN
    Then the response status is 200
    And the response JSON is an array
    And the single-asset grid portions have uniform spacing of 30 seconds

  Scenario: GET /timeline/unknown_asset_xyz returns 404
    When I GET /timeline/unknown_asset_xyz from the VEN
    Then the response status is 404
