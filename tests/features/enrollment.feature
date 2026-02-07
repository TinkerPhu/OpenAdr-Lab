Feature: VEN Program Enrollment
  Programs with targets are only visible to enrolled VENs.
  Programs without targets (open) are visible to all VENs.

  Background:
    Given I have a VTN token as "any-business"

  Scenario: Open program is visible to all VENs
    When I create an open program named "enroll-open-test"
    And I wait for VEN-1 to show program "enroll-open-test"
    And I wait for VEN-2 to show program "enroll-open-test"
    Then VEN-1 has program "enroll-open-test"
    And VEN-2 has program "enroll-open-test"

  Scenario: Targeted program is visible only to enrolled VEN
    When I create a program named "enroll-targeted-test" targeting "ven-1-name"
    And I wait for VEN-1 to show program "enroll-targeted-test"
    Then VEN-1 has program "enroll-targeted-test"
    And VEN-2 does not have program "enroll-targeted-test"
