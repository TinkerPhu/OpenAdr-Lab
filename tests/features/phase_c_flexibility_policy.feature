Feature: FlexibilityPolicy — proactive flexibility reserves

  The planner reduces the effective grid import cap by site-level Up reservations.
  Without policy, a 10.0 kW IMPORT_CAPACITY_LIMIT event yields import_cap_kw=10.0.
  With default_reserve up_kw=3.0, effective cap = 10.0 - 3.0 = 7.0 kW (Layer 1).
  A future SIMPLE event (CP4) adds a further reservation for its window slots only.

  # Layer 1 — always-active default reserve (CP3)

  Scenario: Default reserve reduces effective grid import cap on all firm slots
    Given a VTN IMPORT_CAPACITY_LIMIT event with value 10.0 kW is active
    When I wait for the policy VEN plan to have firm slots with import_cap_kw at most 7.1 kW
    Then every policy VEN firm slot has import_cap_kw at most 7.1 kW
    # Grid cap 10.0 − policy reserve up_kw 3.0 = 7.0 kW effective cap

  # Layer 3 — pre-announced future VTN events (CP4)

  Scenario: Future SIMPLE event reduces import cap for slots inside the event window
    Given a VTN IMPORT_CAPACITY_LIMIT event with value 10.0 kW is active
    And a VTN SIMPLE event with value 5.0 kW starting in 2 hours for 2 hours
    When I wait for the policy VEN plan to have at least one firm slot with import_cap_kw at most 2.1 kW
    Then at least one policy VEN firm slot has import_cap_kw at most 2.1 kW
    # Event window 2h–4h overlaps near horizon (firm_slots cover 0–4h).
    # Grid cap 10.0 − policy reserve 3.0 − SIMPLE reserve 5.0 = 2.0 kW for those slots.

  Scenario: Expired SIMPLE event interval produces no reservation
    Given a VTN IMPORT_CAPACITY_LIMIT event with value 10.0 kW is active
    And a VTN SIMPLE event with value 5.0 kW expired 2 hours ago
    When I wait for the policy VEN plan to have firm slots with import_cap_kw at most 7.1 kW
    Then every policy VEN firm slot has import_cap_kw at most 7.1 kW
    # Expired event excluded → cap = 10.0 − 3.0 (policy only) = 7.0 kW
