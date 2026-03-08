Feature: VEN Rate System — OpenADR Interface (Stage 2)
  The VEN polls the VTN for events and parses rate information
  (PRICE, GHG, EXPORT_PRICE, capacity limits) into structured snapshots
  that are served via dedicated endpoints.

  Background:
    Given I have a VTN token as "any-business"

  Scenario: 3-interval PRICE event produces 3 rate snapshots
    Given I create a rate-system program and save its ID
    And I create a 3-interval PRICE event for the saved program
    When I wait for the VEN /rates endpoint to have at least 3 snapshots
    Then all rate snapshots have an import_price_eur_kwh value

  Scenario: GHG event produces rate snapshots with co2_g_kwh values
    Given I create a rate-system program and save its ID
    And I create a GHG event for the saved program
    When I wait for the VEN /rates endpoint to have at least 1 snapshot
    Then at least one rate snapshot has a co2_g_kwh value

  Scenario: EXPORT_PRICE event produces rate snapshots with export_price_eur_kwh
    Given I create a rate-system program and save its ID
    And I create an EXPORT_PRICE event for the saved program
    When I wait for the VEN /rates endpoint to have at least 1 snapshot
    Then at least one rate snapshot has an export_price_eur_kwh value

  Scenario: IMPORT_CAPACITY_LIMIT event updates the capacity state
    Given I create a rate-system program and save its ID
    And I create an IMPORT_CAPACITY_LIMIT event with limit 5.0 kW for the saved program
    When I wait for the VEN /capacity import_limit_kw to be 5.0
    Then the VEN /capacity response has import_limit_kw equal to 5.0

  Scenario: GET /obligations returns empty list when events have no reportDescriptors
    Given I create a rate-system program and save its ID
    And I create a PRICE event with no reportDescriptors for the saved program
    When I wait for the VEN /rates endpoint to have at least 1 snapshot
    Then the VEN /obligations response is a JSON array

  Scenario: GET /capacity returns a JSON object with expected fields
    When I request GET /capacity from the VEN
    Then the response is a JSON object
    And the response contains the field "last_updated"
