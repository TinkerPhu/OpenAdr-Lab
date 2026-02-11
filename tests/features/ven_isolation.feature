Feature: VEN Isolation
  A VEN must never see another VEN's data through any API call.

  Background:
    Given I have a VTN token as "any-business"
    And I have a VEN-1 token
    And I have a VEN-2 token

  @upstream_pending
  Scenario: VEN can only see its own reports
    Given I create an open program "isolation-report-test" and save its ID
    And I create an event for the saved program named "iso-evt"
    When VEN-1 submits a report to VTN for event "iso-evt" with clientName "ven-1"
    And VEN-2 submits a report to VTN for event "iso-evt" with clientName "ven-2"
    Then VEN-1 querying VTN reports sees only its own reports
    And VEN-2 querying VTN reports sees only its own reports
    And business user querying VTN reports for event "iso-evt" sees both VEN reports

  Scenario: VEN can only see its own VEN record
    When VEN-1 queries VTN for VENs
    Then VEN-1 sees only its own VEN record
    When VEN-2 queries VTN for VENs
    Then VEN-2 sees only its own VEN record

  @upstream_pending
  Scenario: VEN cannot retrieve another VEN's report by ID
    Given I create an open program "isolation-id-test" and save its ID
    And I create an event for the saved program named "iso-id-evt"
    When VEN-1 submits a report to VTN for event "iso-id-evt" with clientName "ven-1"
    And VEN-2 submits a report to VTN for event "iso-id-evt" with clientName "ven-2"
    Then VEN-1 cannot retrieve VEN-2 report by ID
    And VEN-2 cannot retrieve VEN-1 report by ID
