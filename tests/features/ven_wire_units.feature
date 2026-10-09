Feature: The VEN reads a value in the unit it was declared in (GB-50)
  Every event value the VEN acts on is read through what the event (or its
  program) declares about it. A value nobody declared a unit for is read from
  the lab profile's default, and the VEN says that it assumed. A value declared
  in a unit or currency the profile does not read is not used: that payload
  alone is dropped, and GET /health says which event, which payload type, what
  was declared and what the profile reads (docs/reference/WIRE_PROFILE.md,
  "Reading what peers send").

  Background:
    Given I have a VTN token as "bl-client"
    And I create an open program "wire-units-test" and save its ID

  Scenario: A limit whose unit nobody declared is applied, and the VEN says it assumed kW
    Given I send an "IMPORT_CAPACITY_LIMIT" of 4 in force now that declares no unit
    Then the VEN reports an import limit of 4.0 kW in force within 150 seconds
    And the VEN health lists "IMPORT_CAPACITY_LIMIT" under wire assumptions within 60 seconds
    And the VEN health does not blame units for a wire rejection
    When I delete the schedule event

  Scenario Outline: A value declared in a unit the profile does not read is refused, named, and recovers
    Given I send an "<payload type>" of <value> in force now declared as <declaration>
    Then the VEN health reports a wire rejection naming "<named>" within 150 seconds
    And the VEN reports no import limit in force
    When I delete the schedule event
    Then the VEN health stops reporting a units wire rejection within 150 seconds

    Examples:
      | payload type          | value | declaration                         | named        |
      | IMPORT_CAPACITY_LIMIT | 230   | {"units": "VOLTS"}                  | units VOLTS  |
      | PRICE                 | 0.21  | {"units": "KWH", "currency": "USD"} | currency USD |
