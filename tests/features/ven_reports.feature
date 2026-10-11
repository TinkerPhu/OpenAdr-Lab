Feature: VEN Reports
  A VEN can submit a report and it appears in the VTN via the BFF.

  Scenario: Submit report via VEN and verify round-trip
    Given I have a VTN token as "bl-client"
    And I create a program named "report-test-program" and save its ID
    And I create an event for the saved program
    When I wait for VEN-1 to have at least 1 event
    And I submit a report via VEN-1 for the first event
    Then the VEN report submission response status is 201
    And the response body echoes back the submitted report fields
    And the report appears in VEN-1 report list
    And the report appears in BFF report list

  # A report sent by hand can be corrected: the Reports page's edit form PUTs the whole report
  # under the id the VTN gave it, and the VTN's copy is what changes.
  Scenario: Update a submitted report via VEN
    Given I have a VTN token as "bl-client"
    And I create a program named "report-update-program" and save its ID
    And I create an event for the saved program
    When I wait for VEN-1 to have at least 1 event
    And I submit a report via VEN-1 for the first event
    And the report appears in VEN-1 report list
    And I update the submitted report via VEN-1 with a resource named "updated-site"
    Then the VEN report submission response status is 200
    And the BFF report list shows the submitted report with a resource named "updated-site"

  # 3.1 removed `programID` from a report; `eventID` is its only object link,
  # so that is the field whose absence must be refused now. The scenario's
  # subject changed with the protocol -- a body missing the field that
  # identifies what it reports on is still rejected, it is just a different
  # field.
  @ven-unit
  Scenario: POST /reports without the field that names its event is refused
    When I POST to VEN-1 reports with a body missing eventID
    Then the VEN report submission response is a client error
