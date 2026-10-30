# Tasks

## 1. One ranking core in `lab-core`

- [ ] 1.1 Test first in `lab-core`: priority 1 beats none, equal priority newer `createdDateTime` wins, partial overlap splits the lower event, each payload type resolves on its own, a refused-unit payload is skipped and listed. Verify: the tests fail (`wsl` cargo test of `lab-core`, WSL lock held).
- [ ] 1.2 Move `collect_interval_groups` and `resolve_segments` from `VEN/src/controller/rate_schedule.rs` into `lab-core` (`event_priority`), returning segments with the winning event per payload type plus the skipped payloads (design D1, D2). `parse_rate_snapshots` and `parse_capacity_schedule` call it. Verify: 1.1 passes and the existing VEN tests in `openadr_interface.rs` pass unchanged (`cargo test -p ven-app rate capacity`).
- [ ] 1.3 `python ci/audits/audit_ven_architecture.py` and `python ci/audits/audit_file_sizes.py` stay green. Verify: both exit 0.

## 2. The signals feed resolves per VEN

- [ ] 2.1 Test first in `VTN/bff/src/fleet_signals.rs`: a priority-1 PRICE targeting one VEN replaces the unprioritised band for that VEN only; a refused payload still appears with `refused` set. Verify: fails before 2.2.
- [ ] 2.2 `bands_for_ven` (and the route in `routes/fleet.rs`) build bands from the shared core; delete the "priority is deliberately absent" comment. Verify: 2.1 passes, `cargo test` in `VTN/bff`.
- [ ] 2.3 Feed equals VEN: a test resolves one set of overlapping events through the feed and through `parse_rate_snapshots` and compares the winning event for every segment. Verify: the test passes.

## 3. The tariff card

- [ ] 3.1 Test first, `fleetTariff.test.ts`: `tariffGroups` groups by identical price bands whatever the event id, orders by VEN count then first VEN name, leaves VENs without a price out and counts them, caps at eight and reports the rest. Verify: fails before 3.2.
- [ ] 3.2 `utils/fleetTariff.ts`: `tariffGroups` replaces `sharedTariff`; `tariffRows` builds rows for a list of groups; the short-name rule (D5) in one function with its tests (shared prefix cut at `-`/`_`, single group keeps the full name, missing name falls back to count). Verify: 3.1 passes.
- [ ] 3.3 Test first, `FleetTariffChart.test.tsx`: five groups give five import and five export lines, labels like `g3 · 6 VENs`, hover lists the VENs, no dissent note, the basis caption is shown, the draft pair still renders. Verify: fails before 3.4.
- [ ] 3.4 `FleetTariffChart.tsx`: import chart and export chart from the same rows and colours, labels, hover, caption, "n of m VENs" count; the palette is declared once with the other chart colours. Verify: 3.3 and `npm test` in `VTN/ui`, `eslint` zero errors.
- [ ] 3.5 `pages/Fleet.tsx` and `pages/Tariff.tsx` pass the same props; update `Fleet.test.tsx`, `Tariff.test.tsx`, `Signals.test.tsx` where they assumed overlapping bands (D3). Verify: `npm test` green.

## 4. Use case, journal, deploy

- [ ] 4.1 `docs/use-cases/SYSTEM-USE-CASES.md`: operator sends a tariff to some VENs and sees every tariff with its VENs; add a `@ui` scenario in `tests/features/` (two targeted price events, the Fleet page shows two import curves labelled with their VEN counts); run `python ci/audits/audit_step_definitions.py`. Verify: the audit passes.
- [ ] 4.2 Journal entry and KEY_LEARNINGS (the tariff chart showed one group's bands and unranked events while the fleet curve came from five; one ranking implementation for the VEN and the feed). Remove the GB-64 row from `docs/BACKLOG.md`; update `docs/plans/fleet_planning_experiment.md` with the display. Verify: `python ci/audits/audit_debt_gate.py`.
- [ ] 4.3 Hand off with `ci/ci-handoff.sh`; after merge, deploy BFF, VTN UI and the VEN image per the normal process and open the Fleet page with the five experiment events active (or two test events): five labelled curves, the export chart below, the caption. Verify: the page matches the spec scenarios "Five tariffs" and "Short names".

## Workflow follow-up

- Delete this change after merge and wave its behaviour into `docs/architecture/` and `docs/use-cases/` (project workflow rule).
- Out of scope, filed: assign a tariff to VENs in the UI (GB-65); the tariff history for past windows (file if wanted).
