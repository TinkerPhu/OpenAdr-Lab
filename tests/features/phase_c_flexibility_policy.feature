Feature: FlexibilityPolicy — Layer 1 default reserve

  The FlexibilityPolicy module generates Reservation records that reduce per-asset
  available import/export headroom on every planning cycle. A site-level Up reservation
  of 3.0 kW reduces each asset's max_import_kw by 3.0 kW via available_cap().

  Scenario: Default reserve reduces EV import headroom in firm slots
    Given the policy VEN is healthy
    When I wait for the policy VEN plan to have EV firm allocations within headroom
    Then every EV firm allocation grid_power_kw is at most 4.05 kW
    # EV max_charge_kw=7.0, default_reserve up_kw=3.0
    # available_cap("ev").max_import_kw = 7.0 - 3.0 = 4.0 kW
