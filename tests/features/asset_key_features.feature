Feature: Asset key features on the Controller's Flexibility & Forecast panel
  Each asset declares its own key features (label and value, unit included); the VEN serves
  them with the capability it already serves, and the Controller panel prints them in small
  writing under the asset name. Peak power and capacities come straight from the profile; base
  load summarises its recorded history. docs/use-cases/HEMS-USE-CASE-OBSERVATION-MANUAL.md, Controller page.

  Background:
    Given the VEN is running with profile "test"

  Scenario: PV declares its installed peak power from the profile
    When I GET /capability/pv from the VEN
    Then the response status is 200
    And the capability key feature "peak power" is "5.00 kW"

  Scenario: Battery declares its capacity from the profile
    When I GET /capability/battery from the VEN
    Then the response status is 200
    And the capability key feature "capacity" is "10.0 kWh"

  Scenario: EV declares its battery capacity from the profile
    When I GET /capability/ev from the VEN
    Then the response status is 200
    And the capability key feature "capacity" is "60.0 kWh"

  # The numbers depend on how much history the running VEN holds, so the scenario pins the
  # contract (labels, "-" until a record exists, then a kW value), not a figure.
  Scenario: Base load summarises its recorded history over the last two weeks
    When I GET /capability/base_load from the VEN
    Then the response status is 200
    And the capability key feature "avg" is "-" or a kW value
    And the capability key feature "max" is "-" or a kW value
