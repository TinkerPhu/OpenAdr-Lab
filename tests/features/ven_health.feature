Feature: VEN Health Check
  The VEN exposes a health endpoint.

  Scenario: Health endpoint returns ok
    When I GET the VEN "/health" endpoint
    Then the VEN response body is "ok"
