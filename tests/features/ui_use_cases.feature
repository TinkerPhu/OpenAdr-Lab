@ui
Feature: UI Use Cases — Full End-to-End via Browser
  Verify core use cases driven through the VTN Web UI:
  create programs and events via the UI, verify VEN reception via API,
  submit reports via VEN API, verify in UI.

  Background:
    Given previous UI test programs are cleaned up

  Scenario: UI-UC7 - Connectivity Check (open, round-trip)
    Given I open the VTN UI
    When I navigate to the Programs page
    And I create an open program "ui-uc7-connectivity" via the UI
    Then the program "ui-uc7-connectivity" appears in the UI programs list
    When I navigate to the Events page
    And I create a UI event "ui-uc7-heartbeat" for program "ui-uc7-connectivity" with type "SIMPLE" priority 5 and 1 interval
    Then the event "ui-uc7-heartbeat" appears in the UI events table
    When I wait for VEN-1 to show event "ui-uc7-heartbeat"
    And I wait for VEN-2 to show event "ui-uc7-heartbeat"
    Then the VEN-1 event "ui-uc7-heartbeat" has payload type "SIMPLE"
    When I submit a report via VEN-1 for event "ui-uc7-heartbeat"
    Then the report for event "ui-uc7-heartbeat" from "ven-1" appears in VTN
    When I navigate to the Reports page
    Then the report from "ven-1" appears in the UI reports table

  Scenario: UI-UC1 - Emergency Load Shed (targeted to VEN-1 only)
    Given I open the VTN UI
    When I navigate to the Programs page
    And I create a UI program "ui-uc1-emergency" targeting "ven-1"
    Then the program "ui-uc1-emergency" appears in the UI programs list
    When I navigate to the Events page
    And I create a UI event "ui-uc1-loadshed" for program "ui-uc1-emergency" with type "SIMPLE" priority 0 and 1 interval
    Then the event "ui-uc1-loadshed" appears in the UI events table
    When I wait for VEN-1 to show event "ui-uc1-loadshed"
    Then VEN-2 does not have event "ui-uc1-loadshed"
    And the VEN-1 event "ui-uc1-loadshed" has payload type "SIMPLE"
    And the VEN-1 event "ui-uc1-loadshed" has priority 0
    When I submit a report via VEN-1 for event "ui-uc1-loadshed"
    Then the report for event "ui-uc1-loadshed" from "ven-1" appears in VTN
    When I navigate to the Reports page
    Then the report from "ven-1" appears in the UI reports table

  Scenario: UI-UC2 - Export Limitation (targeted to VEN-2 only)
    Given I open the VTN UI
    When I navigate to the Programs page
    And I create a UI program "ui-uc2-export" targeting "ven-2"
    Then the program "ui-uc2-export" appears in the UI programs list
    When I navigate to the Events page
    And I create a UI event "ui-uc2-export-limit" for program "ui-uc2-export" with type "EXPORT_CAPACITY_LIMIT" priority 5 and 3 intervals
    Then the event "ui-uc2-export-limit" appears in the UI events table
    When I wait for VEN-2 to show event "ui-uc2-export-limit"
    Then VEN-1 does not have event "ui-uc2-export-limit"
    And the VEN-2 event "ui-uc2-export-limit" has payload type "EXPORT_CAPACITY_LIMIT"
    And the VEN-2 event "ui-uc2-export-limit" has 3 intervals
    When I submit a report via VEN-2 for event "ui-uc2-export-limit"
    Then the report for event "ui-uc2-export-limit" from "ven-2" appears in VTN

  Scenario: UI-UC3 - Dynamic Pricing (open to all VENs)
    Given I open the VTN UI
    When I navigate to the Programs page
    And I create an open program "ui-uc3-pricing" via the UI
    Then the program "ui-uc3-pricing" appears in the UI programs list
    When I navigate to the Events page
    And I create a UI event "ui-uc3-price" for program "ui-uc3-pricing" with type "PRICE" priority 5 and 3 intervals
    Then the event "ui-uc3-price" appears in the UI events table
    When I wait for VEN-1 to show event "ui-uc3-price"
    And I wait for VEN-2 to show event "ui-uc3-price"
    Then the VEN-1 event "ui-uc3-price" has payload type "PRICE"
    And the VEN-1 event "ui-uc3-price" has 3 intervals

  Scenario: UI-UC4 - Peak Shaving (targeted to VEN-1 and VEN-2)
    Given I open the VTN UI
    When I navigate to the Programs page
    And I create a UI program "ui-uc4-peak" targeting both "ven-1" and "ven-2"
    Then the program "ui-uc4-peak" appears in the UI programs list
    When I navigate to the Events page
    And I create a UI event "ui-uc4-peak-shave" for program "ui-uc4-peak" with type "IMPORT_CAPACITY_LIMIT" priority 3 and 1 interval with intervalPeriod
    Then the event "ui-uc4-peak-shave" appears in the UI events table
    When I wait for VEN-1 to show event "ui-uc4-peak-shave"
    And I wait for VEN-2 to show event "ui-uc4-peak-shave"
    Then the VEN-1 event "ui-uc4-peak-shave" has payload type "IMPORT_CAPACITY_LIMIT"
    And the VEN-1 event "ui-uc4-peak-shave" has an intervalPeriod
    When I submit a report via VEN-1 for event "ui-uc4-peak-shave"
    Then the report for event "ui-uc4-peak-shave" from "ven-1" appears in VTN

  Scenario: UI-UC5 - EV Charging (targeted to VEN-2 with event-level targets)
    Given I open the VTN UI
    When I navigate to the Programs page
    And I create a UI program "ui-uc5-ev" targeting "ven-2"
    Then the program "ui-uc5-ev" appears in the UI programs list
    When I navigate to the Events page
    And I create a UI event "ui-uc5-ev-charge" for program "ui-uc5-ev" with type "IMPORT_CAPACITY_LIMIT" priority 2 and 1 interval with targets
    Then the event "ui-uc5-ev-charge" appears in the UI events table
    When I wait for VEN-2 to show event "ui-uc5-ev-charge"
    Then VEN-1 does not have event "ui-uc5-ev-charge"
    And the VEN-2 event "ui-uc5-ev-charge" has payload type "IMPORT_CAPACITY_LIMIT"
    When I submit a report via VEN-2 for event "ui-uc5-ev-charge"
    Then the report for event "ui-uc5-ev-charge" from "ven-2" appears in VTN

  Scenario: UI-UC6 - Battery Dispatch (targeted to VEN-1 only)
    Given I open the VTN UI
    When I navigate to the Programs page
    And I create a UI program "ui-uc6-battery" targeting "ven-1"
    Then the program "ui-uc6-battery" appears in the UI programs list
    When I navigate to the Events page
    And I create a UI event "ui-uc6-battery-dispatch" for program "ui-uc6-battery" with type "CHARGE_STATE_SETPOINT" priority 3 and 3 intervals
    Then the event "ui-uc6-battery-dispatch" appears in the UI events table
    When I wait for VEN-1 to show event "ui-uc6-battery-dispatch"
    Then VEN-2 does not have event "ui-uc6-battery-dispatch"
    And the VEN-1 event "ui-uc6-battery-dispatch" has payload type "CHARGE_STATE_SETPOINT"
    And the VEN-1 event "ui-uc6-battery-dispatch" has 3 intervals
    When I submit a report via VEN-1 for event "ui-uc6-battery-dispatch"
    Then the report for event "ui-uc6-battery-dispatch" from "ven-1" appears in VTN

  Scenario: UI-UC8 - Event Cancellation (delete via UI, VEN loses event)
    Given I open the VTN UI
    When I navigate to the Programs page
    And I create a UI program "ui-uc8-cancel" targeting "ven-1"
    Then the program "ui-uc8-cancel" appears in the UI programs list
    When I navigate to the Events page
    And I create a UI event "ui-uc8-cancel-evt" for program "ui-uc8-cancel" with type "SIMPLE" priority 5 and 1 interval
    Then the event "ui-uc8-cancel-evt" appears in the UI events table
    When I wait for VEN-1 to show event "ui-uc8-cancel-evt"
    And I delete event "ui-uc8-cancel-evt" via the UI
    Then the event "ui-uc8-cancel-evt" is gone from the UI events table
    When I wait for VEN-1 to no longer show event "ui-uc8-cancel-evt"
