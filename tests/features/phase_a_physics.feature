Feature: Phase A — Asset physics and capability coverage
  Tests for Phase A gaps: capability() state-dependence and UserOverrides paths.
  These scenarios verify existing behaviour that previously had no BDD coverage.

  Background:
    Given the VEN is running with profile "test"
    And the VEN-1 sim overrides are reset
    And the battery SoC is reset to 0.5

  # ── Block A: capability() state-dependence ────────────────────────────────

  Scenario: Battery at full SoC reports zero import capability
    Given the battery SoC is reset to 1.0
    When I GET /capability/battery from the VEN
    Then the response status is 200
    And the capability max_import_kw is 0.0

  Scenario: Battery at empty SoC reports zero export capability
    Given the battery SoC is reset to 0.0
    When I GET /capability/battery from the VEN
    Then the response status is 200
    And the capability max_export_kw is 0.0

  Scenario: EV unplugged reports zero capability in both directions
    When I POST a sim override setting ev_plugged to false
    And I wait 2 seconds for the sim to tick
    And I GET /capability/ev from the VEN
    Then the response status is 200
    And the capability max_import_kw is 0.0
    And the capability max_export_kw is 0.0

  Scenario: PV always reports fixed (non-curtailable) capability
    When I GET /capability/pv from the VEN
    Then the response status is 200
    And the capability is_fixed is true

  # ── Block B: UserOverrides paths ──────────────────────────────────────────

  Scenario: pv_irradiance override to zero silences PV output
    When I POST a sim override setting pv_irradiance to 0.0
    And I wait 2 seconds for the sim to tick
    And I GET /capability/pv from the VEN
    Then the response status is 200
    And the capability is_fixed is true
    And the capability max_import_kw is less than 0.01

  Scenario: pv_irradiance override to full produces nonzero PV export
    When I POST a sim override with full PV irradiance
    And I wait 2 seconds for the sim to tick
    And I GET /capability/pv from the VEN
    Then the response status is 200
    And the capability max_import_kw is less than 0.0

  Scenario: ev_plugged false stops EV charging capability
    When I POST a sim override setting ev_plugged to false
    And I wait 2 seconds for the sim to tick
    And I GET /capability/ev from the VEN
    Then the response status is 200
    And the capability max_import_kw is 0.0
