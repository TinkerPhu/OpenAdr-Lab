Feature: VEN Asset Timeline Endpoints
  Timeline endpoints return merged past-history and future-plan data per asset.

  Background:
    Given the VEN is running

  Scenario: GET /timeline/ev returns a sorted JSON array
    When I GET /timeline/ev from the VEN
    Then the response status is 200
    And the response JSON is an array

  Scenario: GET /timeline/ev response points have ts and values fields
    When I GET /timeline/ev from the VEN
    Then the response status is 200
    And every timeline point has a ts field
    And every timeline point has a values object

  Scenario: GET /timeline/all returns all configured assets and grid
    When I GET /timeline/all from the VEN
    Then the response status is 200
    And the response JSON is an object
    And the timeline all response contains key "ev"
    And the timeline all response contains key "grid"

  Scenario: GET /timeline/grid returns a sorted array
    When I GET /timeline/grid from the VEN
    Then the response status is 200
    And the response JSON is an array

  Scenario: GET /timeline/ev with hours_back=0 returns no past points
    When I GET /timeline/ev?hours_back=0&hours_forward=1 from the VEN
    Then the response status is 200
    And the response JSON is an array
    And all timeline points are at or after now

  Scenario: GET /timeline/ev with extended window returns more points
    When I GET /timeline/ev?hours_back=1&hours_forward=24 from the VEN
    Then the response status is 200
    And the response JSON is an array

  Scenario: GET /timeline/unknown_asset_xyz returns 404
    When I GET /timeline/unknown_asset_xyz from the VEN
    Then the response status is 404
