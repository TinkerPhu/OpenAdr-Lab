Feature: VEN-VTN Integration
  The VEN polls the VTN and reflects programs, events, and sensor data.

  Scenario: VEN reflects programs created in VTN
    Given I have a VTN token as "any-business"
    And I create a program named "ven-poll-program"
    When I wait for the VEN to show program "ven-poll-program"
    Then the VEN program list contains "ven-poll-program"

  Scenario: VEN reflects events created in VTN
    Given I have a VTN token as "any-business"
    And I create a program named "ven-event-program" and save its ID
    And I create an event for the saved program
    When I wait for the VEN to have at least 1 event
    Then the VEN event list is not empty

  Scenario: VEN generates sensor data automatically
    When I wait for the VEN sensor to have power data
    Then the VEN sensor snapshot has a "power_w" value

# TODO: test sensor data entered over UI to reach the VTN by reports