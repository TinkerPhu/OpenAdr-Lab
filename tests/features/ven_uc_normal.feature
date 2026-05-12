Feature: UC-01..UC-04 — Normal Operation Use Cases
  Verify the core HEMS use cases under normal conditions: EV overnight charge lifecycle,
  CONTINUE-policy scheduling, PV surplus tracking, and rate-event-driven plan updates.

  Background:
    Given the VEN is running with profile "test"
    And I set pv plan forecast to 0.0 kW
  # An explicit EV session drives the planner to allocate FIRM charging slots.

  Scenario: UC-01a — Explicit EV session is planned and allocated
    Given I inject ev_soc 0.5 via sim inject
    And I POST an EV session with target_soc 0.90 and departure in 8.0 hours
    When I wait for the VEN /plan to have an EV allocation in slots
    Then at least one firm slot has an allocation for asset "ev"

  Scenario: UC-01b— EV charge plan has FLEXIBLE envelopes for far-horizon energy
    Given I have a VTN token as "any-business"
    And I create a rate-system program and save its ID
    And I create a cheap 4-hour PRICE event for the saved program
    And I POST an EV session with target_soc 0.90 and departure in 8.0 hours
    When I wait for the VEN /plan to have envelopes
    Then the plan has field "envelopes"
    And the plan.envelopes is a non-empty array
    And the plan envelopes contain an entry for asset "ev"

  Scenario: UC-01c — EV packet estimated cost is tracked in the plan
    When I wait for the VEN /plan endpoint to return a plan
    Then the plan has field "summary"

  # --- UC-02: CONTINUE Policy (batch/non-urgent task) ---
  # A request with CONTINUE completion_policy persists past its first deadline
  # and falls back to subsequent tiers. Structure is verified at creation time.

  Scenario: UC-02a — CONTINUE policy request has two deadline tiers
    When I POST a CONTINUE policy request for asset "ev" with two deadline tiers
    Then the response status is 201
    And the response JSON has field "id"
    And the response JSON field "completion_policy" is the string "CONTINUE"
    And the response JSON field "tier_count" is greater than 1.0

  Scenario: UC-02b — CONTINUE policy request appears in /user-requests list
    When I POST a CONTINUE policy request for asset "ev" with two deadline tiers
    And I GET /user-requests from the VEN
    Then the response JSON is an array
    And the requests list has at least 1 item

  # --- UC-03: PV Surplus Cascade ---
  # PV generation self-consumes, charges battery, then exports.
  # Observable: PV energy accumulates in the ledger; battery SoC rises over time.

  Scenario: UC-03 — PV surplus accumulates in the asset ledger
    When I POST a sim override with full PV irradiance
    And I wait for the VEN /plan endpoint to return a plan
    And I poll VEN /ledger until field "pv" is present
    Then the response JSON has field "pv"

  # --- UC-04: Day-Ahead PRICE Event Triggers Rate Update ---
  # VTN posts a multi-interval PRICE event. VEN polls it and populates /rates.
  # The plan is retriggered and uses the new rates.

  Scenario: UC-04a — PRICE event from VTN populates /tariffs with import prices
    Given I have a VTN token as "any-business"
    And I create a rate-system program and save its ID
    And I create a cheap 4-hour PRICE event for the saved program
    When I wait for the VEN /tariffs endpoint to have at least 1 snapshot
    Then all rate snapshots have an import_tariff_eur_kwh value

  Scenario: UC-04b — Plan after PRICE event has rate-priced slots
    Given I inject ev_soc 0.5 via sim inject
    And I POST an EV session with target_soc 0.90 and departure in 12.0 hours
    And I have a VTN token as "any-business"
    And I create a rate-system program and save its ID
    And I create a cheap 4-hour PRICE event for the saved program
    When I wait for the VEN /plan to have an EV allocation in slots
    Then the plan slots have import prices populated
