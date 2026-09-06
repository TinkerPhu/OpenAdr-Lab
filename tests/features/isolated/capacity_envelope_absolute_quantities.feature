Feature: Unified capacity/envelope engine reports absolute quantities — isolated scenarios
  # unified-capacity-envelope-engine (Spec E, asset-max-power-forecast master
  # plan). Both scenarios exercise the live wiring end-to-end (dispatcher
  # tick -> capacity_envelope.rs -> HTTP), not just the Rust unit tests
  # already covering the same logic in isolation, per this repo's `workflow`
  # rule 4 for user-observable behavior changes.

  Background:
    Given the VEN is running with profile "test"

  # ── Site Headroom: absolute, not plan-relative ──────────────────────────

  @isolated
  Scenario: A fully charged battery reports zero absolute import headroom
    Given the battery SoC is reset to 1.0
    When I wait for the VEN site headroom forecast to reflect a full battery
    Then no slot in the site headroom forecast credits the battery with import headroom

  # ── Capacity Forecast: PV-Import and Heater-Export bugs fixed by construction ──

  @isolated
  Scenario: PV generation never inflates the sustained-Import capacity curve
    Given I inject pv irradiance 1.0 via sim inject
    When I wait for the VEN capacity forecast to reflect the injected irradiance
    Then the import capacity curve's first step does not exceed the site's non-PV controllable assets' own import capability
