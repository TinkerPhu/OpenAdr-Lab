Feature: OpenADR Use Cases
  Verify all 8 core use cases work end-to-end: VTN creates events with
  various payload types, VEN receives them with correct structure.

  Background:
    Given I have a VTN token as "any-business"

  Scenario: UC1 - Emergency Load Shed with SIMPLE payload and priority
    Given I create a program named "uc1-emergency-prog" and save its ID
    When I create an event with payload type "SIMPLE" and priority 0
    Then the response status is 201
    And the event response has priority 0
    And the event response has payload type "SIMPLE"

  Scenario: UC2 - Export Limitation with EXPORT_CAPACITY_LIMIT and 3 intervals
    Given I create a program named "uc2-export-prog" and save its ID
    When I create a multi-interval event with payload type "EXPORT_CAPACITY_LIMIT"
    Then the response status is 201
    And the event response has 3 intervals
    And the event response has payload type "EXPORT_CAPACITY_LIMIT"

  Scenario: UC3 - Dynamic Pricing with PRICE payload
    Given I create a program named "uc3-pricing-prog" and save its ID
    When I create an event with payload type "PRICE" and values [0.15]
    Then the response status is 201
    And the event response has payload type "PRICE"

  Scenario: UC4 - Peak Shaving with IMPORT_CAPACITY_LIMIT and intervalPeriod
    Given I create a program named "uc4-peak-prog" and save its ID
    When I create an event with payload type "IMPORT_CAPACITY_LIMIT" and intervalPeriod
    Then the response status is 201
    And the event response has an intervalPeriod
    And the event response has payload type "IMPORT_CAPACITY_LIMIT"

  Scenario: UC5 - EV Charging with targets
    Given I create a program named "uc5-ev-prog" and save its ID
    When I create an event with payload type "IMPORT_CAPACITY_LIMIT" and targets
    Then the response status is 201
    And the event response has targets

  Scenario: UC6 - Battery Dispatch with CHARGE_STATE_SETPOINT
    Given I create a program named "uc6-battery-prog" and save its ID
    When I create an event with payload type "CHARGE_STATE_SETPOINT" and values [80.0]
    Then the response status is 201
    And the event response has payload type "CHARGE_STATE_SETPOINT"

  Scenario: UC7 - Connectivity Check with SIMPLE no-op
    Given I create a program named "uc7-connectivity-prog" and save its ID
    When I create an event with payload type "SIMPLE" and values [0]
    Then the response status is 201

  Scenario: UC8 - Event Cancellation via DELETE
    Given I create a program named "uc8-cancel-prog" and save its ID
    And I create an event with payload type "SIMPLE" and values [1]
    When I delete the created event
    Then the response status is 200
    And the event no longer exists
