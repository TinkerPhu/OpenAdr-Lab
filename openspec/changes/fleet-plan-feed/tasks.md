# Tasks

## 1. VEN publishes the plan

- [ ] 1.1 Find how `tasks/planning.rs` reaches the `TelemetryPort` (or what carries it in `AppCtx`/boot) and where the adopted plan is available; record in design.md. Verify: no "find" left in the inventory.
- [ ] 1.2 Failing test then `plan_wire(&Plan) -> PlanBody`: the projection of spec fleet-plan-publishing (names, kW, non-negative, all slots). Verify: `cargo test -p ven-app plan_wire` green (Node1/2, not local WSL if memory is short).
- [ ] 1.3 `TelemetryPort::publish_plan` with the MQTT publisher (retained, QoS 1) and a no-op/mock implementation in `services/test_support/`. Verify: unit test that the mock receives the plan id after adoption.
- [ ] 1.4 Call it from the planning task after adoption only; failure and absence never change the outcome. Verify: tests "adopt publishes", "reject is silent", "port error does not fail the cycle".
- [ ] 1.5 `python ci/audits/audit_ven_architecture.py` and file-size audit clean. Verify: both exit 0.

## 2. BFF stores the plans

- [ ] 2.1 Failing tests then `fleet::apply_message` handling of the `plan` leaf: a valid body becomes a `PlanRow`, an invalid one is counted and skipped. Verify: `cargo test -p vtn-bff plan` green.
- [ ] 2.2 `lab_recorder.fleet_plan` table, writer queue, insert, retention in `run_retention`. Verify: SQL-free unit tests for row building; table creation exercised by the E2E feature.
- [ ] 2.3 If the plan path repeats the trace path's writer boilerplate a third time, extract the shared part in the same change (one-concept-one-function). Verify: `python ci/audits/audit_duplication.py` unchanged or lower.

## 3. BFF answers "as of an instant"

- [ ] 3.1 Failing tests then `plan_at`: latest plan at or before `at` per VEN; a plan after `at` is ignored; a VEN without a plan is listed, not zero; sum over a shared grid equals the sum of the parts; `at` after the last plan equals now. Verify: `cargo test -p vtn-bff plan_at` green.
- [ ] 3.2 `GET /api/fleet/plan?at=` in `routes/fleet.rs` and `main.rs::api_routes`, response with `contributingVens` per slot and the contributing plan ids. Verify: route unit test for the bad-`at` 400; BDD in 5.1 for the happy path.

## 4. VTN UI

- [ ] 4.1 Client method, hook (`useFleetPlan(at?)`), types, `FleetPlanChart` (planned import and export, kW, shared window helper), a "Fleet plan" card on `pages/Fleet.tsx` with "n of m VENs" and the newest plan's adoption time, and the no-store message. Verify: Vitest for the chart props and the card states; eslint, `tsc`, `npm run build` clean.
- [ ] 4.2 Measure the retained message size and broker memory with 20 VENs on Node2 and settle Open Question 1. Verify: numbers recorded in the journal.

## 5. Use case, BDD, docs

- [ ] 5.1 `tests/features/fleet_plan_feed.feature`: after a VEN adopts a plan, `GET /api/fleet/plan` includes it and its slot import equals the VEN's `GET /plan`; a replan changes the answer for `at=now` but not for an earlier `at`. Verify: `python ci/audits/audit_step_definitions.py`; green on Node2 under the lock.
- [ ] 5.2 Update `docs/plans/fleet-monitor/phase-0-foundation.md` §6.2 (the `plan` row's body is the projection), `docs/architecture/VEN_ARCHITECTURE.md`, `VTN_ARCHITECTURE.md`, `docs/use-cases/`, journal and KEY_LEARNINGS. Verify: files changed in the same commit.
- [ ] 5.3 Full gate by hand on Node1/Node2 (no CI hand-over unless told): audits, VEN unit tests, E2E, resilience. Verify: all green.
