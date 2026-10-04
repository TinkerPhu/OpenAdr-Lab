Feature: VEN User Request Manager — Stage 5
  Users express energy tasks via POST /user-requests (deadline + budget).
  The VEN creates an EnergyPacket, schedules it, and exposes
  results via GET /user-requests, DELETE /user-requests/:id, and GET /flexibility.

  Background:
    Given the VEN is running with profile "test"

  # --- POST /user-requests ---

  Scenario: POST /user-requests creates a user request with a linked EV session
    When I POST a user request for asset "ev" with target_soc 0.90 and latest_end in 12 hours
    Then the response status is 201
    And the response JSON has field "id"
    And the response JSON has field "session_id"
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

  Scenario: Cancelling a user request clears the linked EV session
    When I POST a user request for asset "ev" with target_soc 0.90 and latest_end in 12 hours
    And I save the request ID
    And I DELETE the saved user request
    Then the response status is 204
    And the EV session is cleared after cancellation

  # --- Several charging sessions at once (ev-session-queue) ---

  # The capability itself: a household EV has a sequence of departures, and before
  # the queue a second request simply displaced the first.
  Scenario: A user holds two charging sessions with non-overlapping windows
    When I POST an EV user request available in 1 hours, departing in 8 hours
    Then the response status is 201
    When I POST an EV user request available in 10 hours, departing in 20 hours
    Then the response status is 201
    And the EV session queue holds 2 sessions
    And the queued EV sessions do not overlap

  # The protection: a standing plan silently losing to a spontaneous one is the
  # failure this work exists to prevent, so the clash is refused rather than applied.
  Scenario: An overlapping second session is refused and changes nothing
    When I POST an EV user request available in 1 hours, departing in 8 hours
    Then the response status is 201
    When I POST an EV user request available in 4 hours, departing in 12 hours
    Then the response status is 409
    And the EV session queue holds 1 sessions

  # The user's own case: a standing weekly plan they have stopped thinking about,
  # then a spontaneous trip whose window overlaps it. One of the two deadlines
  # cannot be met - the car is physically away - so the refusal must say which
  # plan is in the way rather than leave them to go and find it.
  Scenario: The refusal names the standing plan it conflicts with
    When I POST an EV user request available in 1 hours, departing in 8 hours
    Then the response status is 201
    When I POST an EV user request available in 4 hours, departing in 12 hours
    Then the response status is 409
    And the refusal names 1 conflicting session
    And the EV session queue holds 1 sessions

  Scenario: Confirming the replacement displaces the standing plan and queues the new one
    When I POST an EV user request available in 1 hours, departing in 8 hours
    Then the response status is 201
    When I POST an EV user request available in 4 hours, departing in 12 hours
    Then the response status is 409
    And the refusal names 1 conflicting session
    When I resubmit that request replacing the named sessions
    Then the response status is 201
    And the EV session queue holds 1 sessions
    And the queued EV sessions do not overlap

  # A confirmation that no longer describes the queue must remove nothing: the
  # whole failure being prevented is a commitment disappearing unnoticed, and a
  # stale instruction is exactly how that would happen one round-trip later.
  Scenario: A replace instruction naming an unqueued session removes nothing
    When I POST an EV user request available in 1 hours, departing in 8 hours
    Then the response status is 201
    When I POST an EV user request available in 4 hours, departing in 12 hours
    Then the response status is 409
    When I resubmit that request replacing a session that is not queued
    Then the response status is 409
    And the EV session queue holds 1 sessions

  # The queue stays usable by hand: no prompt where there is no clash.
  Scenario: A non-overlapping second plan is accepted with no conflict reported
    When I POST an EV user request available in 1 hours, departing in 8 hours
    Then the response status is 201
    When I POST an EV user request available in 10 hours, departing in 20 hours
    Then the response status is 201
    And the EV session queue holds 2 sessions

  # A trip estimate is optional, and it is a pair. Nothing is invented for a user who
  # says nothing: the plan holds the charge flat until the real return is measured.
  Scenario: A session with no trip estimate is accepted and assumes nothing
    When I POST an EV user request available in 1 hours, departing in 8 hours
    Then the response status is 201
    And the queued EV session states no trip

  Scenario: A complete trip estimate is carried on the session
    When I POST an EV user request available in 1 hours, departing in 8 hours, driving 120 km and back in 14 hours
    Then the response status is 201
    And the queued EV session states a trip of 120 km

  # Half an estimate cannot be planned for: a distance with no return time is energy
  # with no instant to apply it to. Completing it by guessing is what this change
  # removed, so it is refused and says which part is missing.
  Scenario: A distance with no return time is refused
    When I POST an EV user request available in 1 hours, departing in 8 hours, driving 120 km with no return time
    Then the response status is 422
    And the refusal says the trip estimate is incomplete
    And the EV session queue holds 0 sessions

  Scenario: A return time with no distance is refused
    When I POST an EV user request available in 1 hours, departing in 8 hours, back in 14 hours with no distance
    Then the response status is 422
    And the refusal says the trip estimate is incomplete
    And the EV session queue holds 0 sessions

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

  # --- Phase F: User leeway ---

  Scenario: Request with tolerance_min and interruptible stores leeway fields
    When I POST a user request with interruptible true and tolerance_min 15 for asset "ev"
    Then the response status is 201
    And the response JSON field "tolerance_min" equals 15.0
    And the response JSON field "interruptible" is true

  Scenario: Budget ceiling via budget_eur is reflected in user request
    When I POST a user request with budget_eur 2.50 for asset "ev"
    Then the response status is 201
    And the response JSON field "max_total_cost_eur" is greater than 0.0

  Scenario: Interruptible scheduled EV session shows up in the site headroom import side
    # Site headroom is absolute, not a delta from current dispatch: a non-V2G
    # EV can't export, so it adds nothing to up_kw however it is charging.
    # What it does add is its own charge ceiling to down_kw while plugged in
    # below its target.
    Given the VEN has a scheduled interruptible EV session
    Then the live site headroom's import side includes the EV's own live import capability
