Feature: Phase A — Asset physics and capability coverage
  Tests for Phase A gaps: capability() state-dependence and UserOverrides paths.
  These scenarios verify existing behaviour that previously had no BDD coverage.

  Background:
    Given the VEN is running with profile "test"
    And the VEN-1 sim overrides are reset
    And the battery SoC is reset to 0.5

  # ── Block A: capability() state-dependence ────────────────────────────────

  # "Battery at full SoC reports zero import capability",
  # "EV unplugged reports zero capability in both directions", and
  # "ev_plugged false stops EV charging capability" (Block B) moved to
  # features/isolated/phase_a_physics_battery.feature (GB-22,
  # docs/BACKLOG.md) — their shared poll_until step raced real backend
  # capability polling under host-load contention during the main pass.

  Scenario: Battery at empty SoC reports zero export capability
    Given the battery SoC is reset to 0.0
    When I GET /capability/battery from the VEN
    Then the response status is 200
    And the capability max_export_kw is 0.0

  Scenario: PV always reports zero import capability (never imports)
    When I GET /capability/pv from the VEN
    Then the response status is 200
    And the capability max_import_kw is 0.0

  # ── Block B: UserOverrides paths ──────────────────────────────────────────

  Scenario: pv_irradiance override to zero silences PV output
    When I POST a sim override setting pv_irradiance to 0.0
    And I wait 5 seconds for the sim to tick
    And I GET /capability/pv from the VEN
    Then the response status is 200
    And the capability max_export_kw magnitude is less than 0.01
    And the capability max_import_kw is less than 0.01

  Scenario: pv_irradiance override to full produces nonzero PV export
    When I POST a sim override with full PV irradiance
    And I wait 5 seconds for the sim to tick
    And I GET /capability/pv from the VEN
    Then the response status is 200
    And the capability max_import_kw is less than 0.0

  Scenario: grid_export_limit_kw sim override actually curtails PV output
    When I POST a sim override with full PV irradiance
    And I POST a sim override setting grid_export_limit_kw to 1.0
    And I wait 5 seconds for the sim to tick
    And I GET /capability/pv from the VEN
    Then the response status is 200
    And the capability max_export_kw magnitude is less than 1.05
