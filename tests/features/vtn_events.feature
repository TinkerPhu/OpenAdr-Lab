Feature: VTN Event Management
  Events can be created for programs and listed via the VTN API.

  Scenario: Create an event for a program
    Given I have a VTN token as "any-business"
    And I create a program named "event-create-program" and save its ID
    When I create an event for the saved program
    Then the response status is 201
    And the response contains a "programID"

  Scenario: List events returns the created event
    Given I have a VTN token as "any-business"
    And I create a program named "event-list-program" and save its ID
    Given I create an event for the saved program
    When I list events
    Then the event list is not empty
