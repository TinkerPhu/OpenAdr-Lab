# Tasks

Operators, chart editing, the plan feed (`fleet-plan-feed`, now documented in
`docs/architecture/VTN_ARCHITECTURE.md`) and the before/after panel are implemented and tested (UI
unit tests; `fleet_plan_feed.feature` on Node2). What remains:

- [ ] 1. A drag on the real chart in a real browser, seen once with the screenshot task of `tariff-editor-workbench` (4.4); `DraftEditLayer.test.tsx` covers the mapping and readout only.
- [ ] 2. Extend `tests/features/tariff_editor.feature`: publish a draft through the editor's path, wait for the VENs' replan, and assert the plan-feed answer for `at=now` differs from `at=publish - 1 s` where the price moved. `fleet_plan_feed.feature` proves the same mechanism with a hand-made PRICE event, not with the editor's publish. Verify: `python ci/audits/audit_step_definitions.py` clean; green on Node1/2 under the lock.
- [ ] 3. Measure the retained `plan` message size and broker memory with 20 VENs on Node2 (open question 1 of the former `fleet-plan-feed` design) and record the numbers in the journal.
