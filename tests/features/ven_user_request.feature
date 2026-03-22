Feature: VEN User Request Manager — Stage 5
  Users express energy tasks via POST /user-requests (deadline + budget).
  The VEN creates an EnergyPacket, schedules it, and exposes
  results via GET /user-requests, DELETE /user-requests/:id, and GET /flexibility.

  Background:
    Given the VEN is running with profile "test"

  # --- POST /user-requests ---

  Scenario: POST /user-requests creates a user request with a linked EV packet
    When I POST a user request for asset "ev" with target_soc 0.90 and latest_end in 12 hours
    Then the response status is 201
    And the response JSON has field "id"
    And the response JSON has field "packet_id"
    And the response JSON field "asset_id" is the string "ev"
    And the response JSON field "status" is the string "ACTIVE"

  Scenario: User request appears in GET /user-requests
    When I POST a user request for asset "ev" with target_soc 0.90 and latest_end in 12 hours
    And I GET /user-requests from the VEN
    Then the response JSON is an array
    And the requests list has at least 1 item

  Scenario: User request with budget constraint includes max_total_cost in linked packet
    When I POST a user request for asset "ev" with target_soc 0.85 and max_cost 3.00 EUR
    Then the response status is 201
    And the response JSON has field "id"
    And the response JSON field "max_total_cost_eur" is greater than 0.0

  Scenario: Multi-tier request has two deadline tiers in the linked packet
    When I POST a multi-tier user request for asset "ev"
    Then the response status is 201
    And the response JSON has field "id"
    And the response JSON field "tier_count" is greater than 1.0

  # --- DELETE /user-requests/:id (cancel) ---

  Scenario: Cancelling a user request abandons the linked packet
    When I POST a user request for asset "ev" with target_soc 0.90 and latest_end in 12 hours
    And I save the request ID
    And I DELETE the saved user request
    Then the response status is 204
    And the cancelled packet is in ABANDONED status

  # --- Non-storage asset rejection ---

  Scenario: Request for a non-storage asset is rejected
    When I POST a user request for asset "pv" with target_soc 0.90 and latest_end in 12 hours
    Then the response status is 422
    And the response JSON has field "error"

  # --- GET /flexibility ---

  Scenario: GET /flexibility returns a site-level flexibility object
    When I GET /flexibility from the VEN
    Then the response status is 200
    And the response JSON contains field "up_kw"
    And the response JSON contains field "down_kw"
