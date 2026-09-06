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
  Scenario: Site headroom forecast never exceeds every asset's own combined live import capability
    # Deliberately does not chase a specific "battery at exactly full SoC"
    # scenario: the live dispatcher keeps ticking (real cost-optimized
    # dispatch can keep discharging the battery a tiny amount every tick),
    # and Battery::capability_inner's soc >= 1.0 ceiling is an EXACT
    # floating-point boundary -- a single tick's worth of drift is enough to
    # flip its reported capability, making "reset to 1.0 then read
    # separately" a genuine, frequent race rather than a flaky assertion to
    # paper over with a looser tolerance. This instead tests the same
    # absolute-vs-relative invariant in a way that's robust to live state:
    # the aggregate down_kw can never exceed what every controllable asset's
    # own /capability endpoint reports at that same moment -- which a
    # plan-relative-delta model (or a double-counting bug) could violate,
    # but the absolute max_effort_setpoint-based model cannot.
    When I wait for the VEN site headroom forecast to be available
    Then the site headroom forecast's first slot does not exceed the site's controllable assets' own combined live import capability

  # ── Capacity Forecast: PV-Import and Heater-Export bugs fixed by construction ──

  @isolated
  Scenario: PV generation never inflates the sustained-Import capacity curve
    Given I inject pv irradiance 1.0 via sim inject
    When I wait for the VEN capacity forecast to reflect the injected irradiance
    Then the import capacity curve's first step does not exceed the site's non-PV controllable assets' own import capability
