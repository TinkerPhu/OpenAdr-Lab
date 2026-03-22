Feature: FlexibilityPolicy — Layer 1 default reserve

  The planner reduces the effective grid import cap by the site-level Up reservation
  on every planning cycle (via site_import_reduction_kw). Without policy, a 10.0 kW
  IMPORT_CAPACITY_LIMIT event yields import_cap_kw=10.0 on firm slots. With
  default_reserve up_kw=3.0, effective cap = 10.0 - 3.0 = 7.0 kW.

  Scenario: Default reserve reduces effective grid import cap on all firm slots
    Given a VTN IMPORT_CAPACITY_LIMIT event with value 10.0 kW is active
    When I wait for the policy VEN plan to have firm slots with import_cap_kw at most 7.1 kW
    Then every policy VEN firm slot has import_cap_kw at most 7.1 kW
    # Grid cap 10.0 − policy reserve up_kw 3.0 = 7.0 kW effective cap

Feature: FlexibilityPolicy — Layer 3 pre-announced VTN events

  Scenario: Future SIMPLE event reduces import cap for slots inside the event window
    Given a VTN IMPORT_CAPACITY_LIMIT event with value 10.0 kW is active
    And a VTN SIMPLE event with value 5.0 kW starting in 4 hours for 2 hours
    When I wait for the policy VEN plan to have at least one firm slot with import_cap_kw at most 2.1 kW
    Then at least one policy VEN firm slot has import_cap_kw at most 2.1 kW
    # Grid cap 10.0 − policy reserve 3.0 − SIMPLE reserve 5.0 = 2.0 kW for window slots

  Scenario: Expired SIMPLE event interval produces no reservation
    Given a VTN IMPORT_CAPACITY_LIMIT event with value 10.0 kW is active
    And a VTN SIMPLE event with value 5.0 kW expired 2 hours ago
    When I wait for the policy VEN plan to have firm slots with import_cap_kw at most 7.1 kW
    Then every policy VEN firm slot has import_cap_kw at most 7.1 kW
    # Expired event excluded → cap = 10.0 − 3.0 (policy only) = 7.0 kW
