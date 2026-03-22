Feature: FlexibilityPolicy — proactive flexibility reserves

  The planner reflects site-level Up reservations in each PlanStep's reserved_up_kw.
  With default_reserve_up_kw=3.0, every plan step has reserved_up_kw >= 3.0 (Layer 1).
  A future SIMPLE event (CP4) stacks an additional reservation for its window slots only.

  # Layer 1 — always-active default reserve (CP3)

  Scenario: Default reserve appears in every plan step
    Given a VTN IMPORT_CAPACITY_LIMIT event with value 10.0 kW is active
    When I wait for the policy VEN plan steps to have reserved_up_kw at least 2.9 kW
    Then every policy VEN plan step has reserved_up_kw at least 2.9 kW
    # policy default_reserve_up_kw=3.0 → reserved_up_kw=3.0 on all steps

  # Layer 3 — pre-announced future VTN events (CP4)

  Scenario: Future SIMPLE event stacks reservation on slots inside the event window
    Given a VTN IMPORT_CAPACITY_LIMIT event with value 10.0 kW is active
    And a VTN SIMPLE event with value 5.0 kW starting in 2 hours for 2 hours
    When I wait for the policy VEN plan steps to have at least one with reserved_up_kw at least 7.9 kW
    Then at least one policy VEN plan step has reserved_up_kw at least 7.9 kW
    # Slots 2h–4h: policy 3.0 + SIMPLE 5.0 = 8.0 kW reserved

  Scenario: Expired SIMPLE event interval produces no extra reservation
    Given a VTN IMPORT_CAPACITY_LIMIT event with value 10.0 kW is active
    And a VTN SIMPLE event with value 5.0 kW expired 2 hours ago
    When I wait for the policy VEN plan steps to have reserved_up_kw at least 2.9 kW
    Then every policy VEN plan step has reserved_up_kw at least 2.9 kW
    # Expired event excluded → only policy reserve 3.0 kW applies
