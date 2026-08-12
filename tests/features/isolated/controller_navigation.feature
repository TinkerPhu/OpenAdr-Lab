Feature: Controller V2 — Navigation and Layout Controls — isolated scenarios
  # GB-22 (docs/BACKLOG.md): un-isolated, this scenario races real backend
  # calls (health/capability polling) during the main E2E pass and can fail
  # under host-load contention with browser 502/404 console errors, even
  # though the underlying collapse/expand behavior is correct — it passes
  # cleanly every time run in isolation.

  Background:
    Given I have a VTN token as "any-business"
    And I open the VEN-1 controller V2 UI

  @isolated @ven-ui
  Scenario: Right section starts collapsed and can be expanded then collapsed
    Then the EV asset cell right section is not visible
    When I click the collapse right button on the EV asset cell
    Then the EV asset cell right section is visible
    When I click the collapse right button on the EV asset cell
    Then the EV asset cell right section is not visible
