Feature: BFF VEN Access
  VENs can be listed via the BFF API.

  Scenario: List VENs via BFF
    When I list VENs via BFF
    Then the response status is 200

  Scenario: BFF health includes VTN status
    When I GET BFF health
    Then the BFF health shows VTN reachable
