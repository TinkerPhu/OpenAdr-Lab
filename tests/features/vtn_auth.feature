Feature: VTN Authentication
  The VTN issues OAuth tokens via client_credentials grant.

  Scenario: Valid credentials return an access token
    Given I request a token with client_id "any-business" and client_secret "any-business"
    Then the response status is 200
    And the response contains an "access_token"

  Scenario: Invalid credentials are rejected
    Given I request a token with client_id "bogus" and client_secret "wrong"
    Then the response status is not 200
