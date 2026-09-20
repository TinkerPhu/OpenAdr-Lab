Feature: VEN Program Enrollment
  Programs with targets are only visible to enrolled VENs.
  Programs without targets (open) are visible to all VENs.

  Background:
    Given I have a VTN token as "bl-client"

  Scenario: Open program is visible to all VENs
    When I create an open program named "enroll-open-test"
    And I wait for VEN-1 to show program "enroll-open-test"
    And I wait for VEN-2 to show program "enroll-open-test"
    Then VEN-1 has program "enroll-open-test"
    And VEN-2 has program "enroll-open-test"

  Scenario: Targeted program is visible only to enrolled VEN
    When I create a program named "enroll-targeted-test" targeting "ven-1"
    And I wait for VEN-1 to show program "enroll-targeted-test"
    Then VEN-1 has program "enroll-targeted-test"
    And VEN-2 does not have program "enroll-targeted-test"

  # Target hiding (3.1 Definition: "for program and event objects, a VTN will
  # only include requested targets in a response"). A VEN must not learn who
  # else an object addresses -- on a shared VTN the target list is the fleet's
  # membership, and leaking it tells every VEN about every other.
  #
  # This is enforced by the VTN's own `retrieve_all_with_client_id`, which
  # redacts via `intersection()`. It has its own upstream tests; what this adds
  # is the lab's end of the contract, since it was our fork patch P-4 that used
  # to do this and that patch is now retired in favour of the native path
  # (docs/reference/FORK_PATCHES.md). A silent regression here would look
  # exactly like working target filtering.
  Scenario: A VEN sees only itself in a shared object's target list
    Given I have a VTN token as "bl-client"
    When I create a program named "target-hiding-test" targeting both "ven-1" and "ven-2"
    Then reading it as "ven-1" shows targets exactly "ven-1"
    And reading it as "ven-2" shows targets exactly "ven-2"
    And reading it as "bl-client" shows targets "ven-1" and "ven-2"
