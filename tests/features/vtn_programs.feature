Feature: VTN Program Management
  Programs can be created and listed via the VTN API.

  Background:
    Given I have a VTN token as "any-business"

  Scenario: Create a program
    When I create a program named "test-program-1"
    Then the response status is 201
    And the response contains "programName" equal to "test-program-1"

  Scenario: List programs includes the created program
    Given I create a program named "test-program-list"
    When I list programs
    Then the program list contains "test-program-list"

  Scenario: Unauthenticated request is rejected
    When I GET "/programs" without authentication
    Then the response status is 401
