Feature: BFF Program CRUD
  Programs can be created, updated, and deleted via the BFF API.

  Scenario: Create a program via BFF
    When I create a program via BFF named "bff-test-program"
    Then the response status is 200
    And the response contains "programName" equal to "bff-test-program"

  Scenario: Update a program via BFF
    Given I create a program via BFF named "bff-rename-me"
    When I update the program name to "bff-renamed"
    Then the response status is 200
    And the response contains "programName" equal to "bff-renamed"

  Scenario: Delete a program via BFF
    Given I create a program via BFF named "bff-delete-me"
    When I delete the program via BFF
    Then the response status is 200
    And the program no longer appears in the BFF program list
