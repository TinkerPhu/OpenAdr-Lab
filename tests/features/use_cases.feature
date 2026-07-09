Feature: OpenADR Use Cases — Full End-to-End
  Verify all 8 core use cases end-to-end: enrollment targeting, event creation,
  VEN reception with correct structure, report submission, and cancellation.

  Background:
    Given I have a VTN token as "any-business"

  Scenario: UC1 - Emergency Load Shed (targeted to VEN-1 only)
    Given I create a program "uc1-e2e-emergency" targeting "ven-1" and save its ID
    When I create a UC event "uc1-e2e-loadshed" with type "SIMPLE" priority 0 and 1 interval
    Then the response status is 201
    When I wait for VEN-1 to show event "uc1-e2e-loadshed"
    Then VEN-2 does not have event "uc1-e2e-loadshed"
    And the VEN-1 event "uc1-e2e-loadshed" has payload type "SIMPLE"
    And the VEN-1 event "uc1-e2e-loadshed" has priority 0
    When I submit a report via VEN-1 for event "uc1-e2e-loadshed"
    Then the report for event "uc1-e2e-loadshed" from "ven-1" appears in VTN

  Scenario: UC2 - Export Limitation (targeted to VEN-2 only)
    Given I create a program "uc2-e2e-export" targeting "ven-2" and save its ID
    When I create a UC event "uc2-e2e-export-limit" with type "EXPORT_CAPACITY_LIMIT" priority 5 and 3 intervals
    Then the response status is 201
    When I wait for VEN-2 to show event "uc2-e2e-export-limit"
    Then VEN-1 does not have event "uc2-e2e-export-limit"
    And the VEN-2 event "uc2-e2e-export-limit" has payload type "EXPORT_CAPACITY_LIMIT"
    And the VEN-2 event "uc2-e2e-export-limit" has 3 intervals
    When I submit a report via VEN-2 for event "uc2-e2e-export-limit"
    Then the report for event "uc2-e2e-export-limit" from "ven-2" appears in VTN

  Scenario: UC3 - Dynamic Pricing (open to all VENs)
    Given I create an open program "uc3-e2e-pricing" and save its ID
    When I create a UC event "uc3-e2e-price" with type "PRICE" priority 5 and 3 intervals
    Then the response status is 201
    When I wait for VEN-1 to show event "uc3-e2e-price"
    And I wait for VEN-2 to show event "uc3-e2e-price"
    Then the VEN-1 event "uc3-e2e-price" has payload type "PRICE"
    And the VEN-1 event "uc3-e2e-price" has 3 intervals

  Scenario: UC4 - Peak Shaving (targeted to VEN-1 and VEN-2)
    Given I create a program "uc4-e2e-peak" targeting both "ven-1" and "ven-2" and save its ID
    When I create a UC event "uc4-e2e-peak-shave" with type "IMPORT_CAPACITY_LIMIT" priority 3 and 1 interval with intervalPeriod
    Then the response status is 201
    When I wait for VEN-1 to show event "uc4-e2e-peak-shave"
    And I wait for VEN-2 to show event "uc4-e2e-peak-shave"
    Then the VEN-1 event "uc4-e2e-peak-shave" has payload type "IMPORT_CAPACITY_LIMIT"
    And the VEN-1 event "uc4-e2e-peak-shave" has an intervalPeriod
    When I submit a report via VEN-1 for event "uc4-e2e-peak-shave"
    Then the report for event "uc4-e2e-peak-shave" from "ven-1" appears in VTN

  Scenario: UC5 - EV Charging (targeted to VEN-2 with event-level targets)
    Given I create a program "uc5-e2e-ev" targeting "ven-2" and save its ID
    When I create a UC event "uc5-e2e-ev-charge" with type "IMPORT_CAPACITY_LIMIT" priority 2 and 1 interval with targets
    Then the response status is 201
    When I wait for VEN-2 to show event "uc5-e2e-ev-charge"
    Then VEN-1 does not have event "uc5-e2e-ev-charge"
    And the VEN-2 event "uc5-e2e-ev-charge" has payload type "IMPORT_CAPACITY_LIMIT"
    When I submit a report via VEN-2 for event "uc5-e2e-ev-charge"
    Then the report for event "uc5-e2e-ev-charge" from "ven-2" appears in VTN

  Scenario: UC6 - Battery Dispatch (targeted to VEN-1 only)
    Given I create a program "uc6-e2e-battery" targeting "ven-1" and save its ID
    When I create a UC event "uc6-e2e-battery-dispatch" with type "CHARGE_STATE_SETPOINT" priority 3 and 3 intervals
    Then the response status is 201
    When I wait for VEN-1 to show event "uc6-e2e-battery-dispatch"
    Then VEN-2 does not have event "uc6-e2e-battery-dispatch"
    And the VEN-1 event "uc6-e2e-battery-dispatch" has payload type "CHARGE_STATE_SETPOINT"
    And the VEN-1 event "uc6-e2e-battery-dispatch" has 3 intervals
    When I submit a report via VEN-1 for event "uc6-e2e-battery-dispatch"
    Then the report for event "uc6-e2e-battery-dispatch" from "ven-1" appears in VTN

  Scenario: UC7 - Connectivity Check (open, no-op round-trip)
    Given I create an open program "uc7-e2e-connectivity" and save its ID
    When I create a UC event "uc7-e2e-heartbeat" with type "SIMPLE" priority 5 and 1 interval
    Then the response status is 201
    When I wait for VEN-1 to show event "uc7-e2e-heartbeat"
    And I wait for VEN-2 to show event "uc7-e2e-heartbeat"
    Then the VEN-1 event "uc7-e2e-heartbeat" has payload type "SIMPLE"
    When I submit a report via VEN-1 for event "uc7-e2e-heartbeat"
    Then the report for event "uc7-e2e-heartbeat" from "ven-1" appears in VTN

  Scenario: UC8 - Event Cancellation (VEN-1 sees then loses event)
    Given I create a program "uc8-e2e-cancel" targeting "ven-1" and save its ID
    When I create a UC event "uc8-e2e-cancel-evt" with type "SIMPLE" priority 5 and 1 interval
    Then the response status is 201
    When I wait for VEN-1 to show event "uc8-e2e-cancel-evt"
    And I delete event "uc8-e2e-cancel-evt"
    Then the response status is 200
    When I wait for VEN-1 to no longer show event "uc8-e2e-cancel-evt"

  # ── Extended coverage scenarios ──────────────────────────────────────────

  Scenario: UC3b - Day-ahead pricing with 24 hourly intervals
    Given I create an open program "uc3b-e2e-24h" and save its ID
    When I create a UC event "uc3b-e2e-24h-price" with type "PRICE" priority 5 and 24 intervals
    Then the response status is 201
    When I wait for VEN-1 to show event "uc3b-e2e-24h-price"
    And I wait for VEN-2 to show event "uc3b-e2e-24h-price"
    Then the VEN-1 event "uc3b-e2e-24h-price" has 24 intervals
    And the VEN-1 event "uc3b-e2e-24h-price" has payload type "PRICE"

  Scenario: UC3c - Price correction after initial publish
    Given I create an open program "uc3c-e2e-correction" and save its ID
    When I create a UC event "uc3c-e2e-orig" with type "PRICE" priority 5 and 1 interval
    Then the response status is 201
    When I wait for VEN-1 to show event "uc3c-e2e-orig"
    Then the VEN-1 event "uc3c-e2e-orig" has payload value 0
    When I update event "uc3c-e2e-orig" with type "PRICE" and value 0.99
    And I wait for VEN-1 event "uc3c-e2e-orig" to have payload value 0.99
    Then the VEN-1 event "uc3c-e2e-orig" has payload value 0.99

  Scenario: UC4b - Modify peak shaving limit mid-flight
    Given I create a program "uc4b-e2e-modify" targeting "ven-1" and save its ID
    When I create a UC event "uc4b-e2e-peak" with type "IMPORT_CAPACITY_LIMIT" priority 3 and 1 interval with intervalPeriod
    Then the response status is 201
    When I wait for VEN-1 to show event "uc4b-e2e-peak"
    Then the VEN-1 event "uc4b-e2e-peak" has payload value 10000
    When I update event "uc4b-e2e-peak" with type "IMPORT_CAPACITY_LIMIT" and value 30
    And I wait for VEN-1 event "uc4b-e2e-peak" to have payload value 30
    Then the VEN-1 event "uc4b-e2e-peak" has payload value 30

  Scenario: UC5b - Overlapping EV events with different priorities
    Given I create a program "uc5b-e2e-overlap" targeting "ven-2" and save its ID
    When I create a UC event "uc5b-e2e-high" with type "IMPORT_CAPACITY_LIMIT" priority 2 and 1 interval
    Then the response status is 201
    When I create a UC event "uc5b-e2e-low" with type "IMPORT_CAPACITY_LIMIT" priority 4 and 1 interval
    Then the response status is 201
    When I wait for VEN-2 to show event "uc5b-e2e-high"
    And I wait for VEN-2 to show event "uc5b-e2e-low"
    Then the VEN-2 event "uc5b-e2e-high" has priority 2
    And the VEN-2 event "uc5b-e2e-low" has priority 4

  Scenario: UC6b - Conflicting charge and discharge events
    Given I create a program "uc6b-e2e-conflict" targeting "ven-1" and save its ID
    When I create a UC event "uc6b-e2e-charge" with type "CHARGE_STATE_SETPOINT" priority 3 and value 80
    Then the response status is 201
    When I create a UC event "uc6b-e2e-discharge" with type "CHARGE_STATE_SETPOINT" priority 2 and value -50
    Then the response status is 201
    When I wait for VEN-1 to show event "uc6b-e2e-charge"
    And I wait for VEN-1 to show event "uc6b-e2e-discharge"
    Then VEN-1 has 2 events matching prefix "uc6b-e2e-"
    And the VEN-1 event "uc6b-e2e-charge" has payload value 80
    And the VEN-1 event "uc6b-e2e-discharge" has payload value -50
    And the VEN-1 event "uc6b-e2e-discharge" has priority 2
    And the VEN-1 event "uc6b-e2e-charge" has priority 3
