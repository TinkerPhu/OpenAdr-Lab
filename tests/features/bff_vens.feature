Feature: BFF VEN Access
  VENs can be listed via the BFF API.

  Scenario: List VENs via BFF
    When I list VENs via BFF
    Then the response status is 200

  Scenario: BFF health includes VTN status
    When I GET BFF health
    Then the BFF health shows VTN reachable

  # BL-41: a VEN on a different physical host than the fleet dashboard
  # advertises its own reachable origin via a DASHBOARD_URL attribute. The
  # dashboard reads this through the BFF's VEN list, so the attribute must
  # round-trip unmodified end-to-end (VTN -> BFF -> dashboard). Both VENs are
  # registered before the single BFF list call so the BFF's GET /vens cache
  # (CACHE_TTL_VENS) is populated once with both, not raced between scenarios.
  Scenario: VEN dashboard address resolves from DASHBOARD_URL when present, else is unaffected
    Given I register a VEN named "bl41-dashboard-ven" with a DASHBOARD_URL attribute of "http://192.168.1.104:8211"
    And I register a VEN named "bl41-same-host-ven" with no DASHBOARD_URL attribute
    When I list VENs via BFF
    Then the response status is 200
    And the listed VEN "bl41-dashboard-ven" has a DASHBOARD_URL attribute of "http://192.168.1.104:8211"
    And the listed VEN "bl41-same-host-ven" has no DASHBOARD_URL attribute

  # R5: in 3.1 `clientID` is what identifies a VEN object to the VTN, and the
  # alias trap makes getting it wrong easy and quiet. `write_vens` is an alias
  # for `write_vens_ven`, which *overwrites* the clientID in the body with the
  # caller's own token subject -- so a business client provisioning the fleet
  # with the alias stamps its own identity onto every VEN, and the second
  # creation collides on ven_client_id_unique rather than saying what happened.
  # A fleet where two VENs share a clientID is indistinguishable from a healthy
  # one until something is addressed to the wrong VEN.
  Scenario: Every VEN is identified by its own clientID
    When I list VENs via BFF
    Then the response status is 200
    And no two VENs share a clientID
    And no VEN is identified by the provisioning client
