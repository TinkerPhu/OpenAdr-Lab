# Tasks

## 1. The known events

- [ ] 1.1 Test first in `detect.rs` (`event_poll_tests`): an event with the same id and a different
  `modificationDateTime` and different prices arrives again (`OpenAdrArrived` carrying the new version) and
  `plan_trigger()` is `RateChange`; an event with the same id and version does not arrive; an edited then deleted
  event expires. Verify: the first test fails (`wsl` cargo test of `ven-app`, WSL lock held, `-j 2`).
- [ ] 1.2 `tasks/poll_events/known_events.rs`: `KnownEvents` (id to version) with `from_events`, `is_new`, `ids`
  and its own tests (same id and version known, new id new, new version new, an earlier version new). Verify: its
  tests pass.
- [ ] 1.3 `detect_event_changes` takes `&KnownEvents` and returns `known` in `EventChanges` (replacing
  `current_ids`); the arrival loop uses `is_new`, the expiry loop `ids()`; `spawn_event_poll` keeps `prev_known` and
  hands `ids()` to `retire_obligations_not_in`. Update the existing `event_poll_tests` fixtures. Verify: 1.1 passes
  and every test under `tasks/poll_events` passes with its meaning unchanged.
- [ ] 1.4 `python ci/audits/audit_file_sizes.py` (`tasks/` stays at or under 200 production lines) and
  `python ci/audits/audit_ven_architecture.py`. Verify: both exit 0.

## 2. Capacity, alerts and attribution

- [ ] 2.1 Test first: an edited `IMPORT_CAPACITY_LIMIT` event yields both `OpenAdrArrived` and `CapacityChange`
  and the trigger is `CapacityChange`; an edited alert event counts as arrived; the poll's `trigger_causes` lists
  the edited event id. Verify: fails before 1.3 and passes after.
- [ ] 2.2 `events_received` gets a record per version (`event_records` follows the arrival loop). Verify: a test
  that two versions of one id give two records.

## 3. Use case and BDD

- [ ] 3.1 `docs/use-cases/SYSTEM-USE-CASES.md` (the tariff editor and fleet sections): a tariff edited in place
  reaches the VENs' plans within a poll. Add a scenario to `tests/features/` that publishes a price event, lets the
  VEN plan, updates the event's prices through the VTN API and sees the VEN's trace show a second arrival with a new
  `modification_date_time` and a plan whose tariff is the new one. Reuse the existing event-publishing and trace
  steps (`ls tests/features/steps/`) and run `python ci/audits/audit_step_definitions.py`. Verify: the audit passes.
- [ ] 3.2 Journal entry and KEY_LEARNINGS (an edit in place keeps the id and the interval count: identify an object
  by its version when the peer sends one). Remove the GB-63 row from `docs/BACKLOG.md`. Verify:
  `python ci/audits/audit_debt_gate.py`.

## 4. Hand-off

- [ ] 4.1 `ci-handoff` (the runner selects `cargo`, `e2e`, `resilience`). After the merge, deploy the VEN image to
  ven-1..3 (Node1) and ven-4..20 (Node2) when the user asks, then `PUT` a price on the live VTN and look at a VEN's
  Planner page. Verify: the page shows two arrivals of one event id.

## Workflow follow-up

- Delete this change after the merge and wave its behaviour into `docs/architecture/VEN_ARCHITECTURE.md` (the event
  poll section) and the use-case page.
