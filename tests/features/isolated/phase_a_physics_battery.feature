Feature: Phase A — Asset physics capability polling — isolated scenarios
  # GB-22 (docs/BACKLOG.md): un-isolated, these scenarios race real backend
  # capability polling (poll_until, 120s timeout) during the main E2E pass
  # and can time out under host-load contention even though the underlying
  # logic is correct — confirmed: fails under load ~7.3-7.7, passes in
  # 0.06s under load ~1.8.

  Background:
    Given the VEN is running with profile "test"
    And the VEN-1 sim overrides are reset
    And the battery SoC is reset to 0.5

  @isolated
  Scenario: Battery at full SoC reports zero import capability
    Given the battery SoC is reset to 1.0
    When I wait for the VEN /capability/battery max_import_kw to equal 0.0
    Then the polled capability matched

  @isolated
  Scenario: EV unplugged reports zero capability in both directions
    When I POST a sim override setting ev_plugged to false
    And I wait for the VEN /capability/ev max_import_kw to equal 0.0
    And I wait for the VEN /capability/ev max_export_kw to equal 0.0
    Then the polled capability matched

  @isolated
  Scenario: ev_plugged false stops EV charging capability
    When I POST a sim override setting ev_plugged to false
    And I wait for the VEN /capability/ev max_import_kw to equal 0.0
    Then the polled capability matched
