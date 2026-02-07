Feature: BFF Reports
  Reports can be listed via the BFF API.

  Scenario: List reports via BFF
    When I list reports via BFF
    Then the response status is 200
    And the response is a JSON array
