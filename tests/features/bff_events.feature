Feature: BFF Event CRUD
  Events can be created, updated, and deleted via the BFF API.

  Scenario: Create an event via BFF
    Given I create a program via BFF named "bff-event-program" and save its ID
    When I create an event via BFF for the saved program named "bff-test-event"
    Then the response status is 200
    And the response contains "eventName" equal to "bff-test-event"

  Scenario: Delete an event via BFF
    Given I create a program via BFF named "bff-event-del-prog" and save its ID
    And I create an event via BFF for the saved program named "bff-del-event"
    When I delete the event via BFF
    Then the response status is 200
    And the event no longer appears in the BFF event list
