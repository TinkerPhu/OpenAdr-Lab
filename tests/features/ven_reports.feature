Feature: VEN Reports
  A VEN can submit a report and it appears in the VTN via the BFF.

  Scenario: Submit report via VEN and verify round-trip
    Given I have a VTN token as "any-business"
    And I create a program named "report-test-program" and save its ID
    And I create an event for the saved program
    When I wait for VEN-1 to have at least 1 event
    And I submit a report via VEN-1 for the first event
    Then the VEN report submission response status is 201
    And the report appears in VEN-1 report list
    And the report appears in BFF report list
