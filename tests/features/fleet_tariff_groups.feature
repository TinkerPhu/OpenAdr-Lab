@fleet @tariff
Feature: The Fleet page shows the tariff each VEN is under (GB-64)
  An operator can send a tariff to some VENs only. The tariff drawn beside the fleet power must be the
  one each VEN acts on: where events overlap, the one that outranks the others, and a tariff reaching
  only some VENs must be shown as its own curve with the VENs on it, not folded into the majority's.

  Background:
    Given I have a VTN token as "bl-client"
    And I create an open program "fleet-tariff-groups-test" and save its ID
    When I wait for the fleet feed to have at least 2 VEN

  Scenario: A priority event replaces the open price for its target VEN only
    Given a PRICE event "tg-standard" of 0.25 with no priority for every VEN
    And a priority-1 PRICE event "tg-override" of 0.12 for the first reporting VEN
    When I wait for the fleet signals of the last 10 minutes to show both PRICE events
    Then the first reporting VEN's price bands come only from "tg-override"
    And another VEN's price bands include "tg-standard" and not "tg-override"

  @ui
  Scenario: The Fleet page lists the targeted tariff with its VEN
    Given I open the VTN UI
    And a PRICE event "tg-standard" of 0.25 with no priority for every VEN
    And a priority-1 PRICE event "tg-override" of 0.12 for the first reporting VEN
    When I wait for the fleet signals of the last 10 minutes to show both PRICE events
    And I navigate to the Fleet page
    Then the Fleet tariff card lists a tariff of "tg-override" for the first reporting VEN
    And the Fleet tariff card says it shows the VTN's events as of now
