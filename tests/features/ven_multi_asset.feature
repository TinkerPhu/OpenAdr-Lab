Feature: Several assets of one kind (W7)
  A VEN may hold several assets of one kind — here an east and a west PV array and two EVs.
  Each is listed, planned, forecast and steered under its own id, and a request or a setting
  for one EV leaves the other alone.

  Background:
    Given the VEN is running with profile "multi_asset_test"

  Scenario: Every asset is listed under its own id with its declared type
    When I GET /sim from the VEN
    Then the VEN lists the assets "car-a" and "car-b" of type "ev"
    And the VEN lists the assets "pv-east" and "pv-west" of type "pv"

  Scenario: Each PV array is forecast separately in the plan
    Then within 120 seconds the plan forecasts the arrays "pv-east" and "pv-west" each on its own

  Scenario: A request for one EV becomes that EV's session
    When I POST a user request for asset "car-b" with target_soc 0.8 and latest_end in 6 hours
    Then the response status is 201
    And the EV "car-b" holds the new session and the EV "car-a" holds none

  Scenario: One EV's opportunistic-charging setting leaves the other alone
    When I turn opportunistic charging off for EV "car-a"
    And I GET /assets/car-a/ev-settings from the VEN
    Then the response JSON field "opportunistic_charging_enabled" is false
    When I GET /assets/car-b/ev-settings from the VEN
    Then the response JSON field "opportunistic_charging_enabled" is true
    When I turn opportunistic charging on for EV "car-a"

  # R-143: the VTN asks for STORAGE_CHARGE_LEVEL without `aggregate` (spec default false), so
  # every storage asset reports as its own resource, whatever its id.
  Scenario: Each storage asset reports its own state of charge
    Given I have a VTN token as "bl-client"
    And I create a program named "multi-soc-test" and save its ID
    And I create an event for the saved program with a reportDescriptor of type "STORAGE_CHARGE_LEVEL" reporting every 5 seconds
    Then within 120 seconds a report for the event names the resources "ven-multi-battery", "ven-multi-car-a" and "ven-multi-car-b"
