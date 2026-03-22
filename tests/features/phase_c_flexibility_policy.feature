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
