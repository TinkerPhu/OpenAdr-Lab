Feature: BFF Reports
  Reports can be listed via the BFF API.

  Scenario: List reports via BFF
    When I list reports via BFF
    Then the response status is 200
    And the response is a JSON array

  # The wire contract a report has to satisfy from 3.1 on. Every clause here
  # is something this lab got wrong at some point and a unit test alone did not
  # catch, because each is about what actually reaches the VTN:
  #   * `programID` was removed by 3.1; `eventID` is a report's only object link
  #   * `payloadDescriptors` is optional in the spec and mandatory here -- a
  #     value whose unit lives only in the reader's head is GB-50
  #   * `USAGE` is energy over an interval, so it is declared KWH and its
  #     interval states the window it covers
  Scenario: Reports on the wire declare what their values mean
    When I wait for the fleet to have reported
    Then the response status is 200
    And every report omits "programID"
    And every report names its event
    And every USAGE payload is declared as "KWH"
    And every USAGE interval states the window it covers
