# Tasks

## 1. Search and registry

- [x] 1.1 Grep for existing ISO 8601 duration parsing/formatting, priority arithmetic and payload-unit text in `VTN/ui/src` and `lab-core`; record in design.md inventory what is reused. Verify: the inventory table has no "check" row left.
- [x] 1.2 (registry module, pure parts tested; `useSignalRegistry` is exercised by the page test in 4.3) Write failing Vitest for `signalRegistry.ts`: the entry type, lookup that rejects an unknown type, and `useSignalRegistry` reading `GET /api/signals/registry` (unit/currency come from the BFF, task 3.3, so the UI holds no second copy of the PRICE→KWH/EUR table that `wire_contract.rs` and `seed_vtn.py` already hold). UI-only facts (label, step, min, max) are declared in the same file as data. Verify: `cd VTN/ui && npm test -- signalRegistry` green.

## 2. Draft model and recurrence (test first)

- [x] 2.1 Failing tests then `recurrence.ts`: `P9999Y`→forever, `P7D`→span, shorter→truncated, absent→once, and inverse mapping. Verify: `npm test -- recurrence` green.
- [x] 2.2 Failing tests then `signalDraft.ts`: set/fill/paste (24 lines), validation (non-number, range, gap in repeating pattern, non-positive period), `fromEvent` → `toEventInput` round trip of the seeded `tou-pricing-day-ahead` shape yields identical `duration` and intervals. Verify: `npm test -- signalDraft` green.
- [x] 2.3 `toEventInput` always emits the registry's payload descriptors and the D6 priority; test that an undeclared payload cannot be built. Verify: the test fails if the descriptor line is removed.

## 3. BFF preview

- [x] 3.1 Failing Rust test then `POST /api/signals/preview`: a 24-interval `P9999Y` draft over a 48 h window returns two days of bands equal to `bands_for_ven` for the same event. Verify: `cargo test -p vtn-bff preview` (WSL, `-j 2`, under `wsl_lock`).
- [x] 3.1a The same endpoint with `pattern: true` returns the event's unexpanded timed intervals (`lab_core::event_timing::base_intervals` made `pub`; no new timing logic). Verify: Rust test that an interval without its own start follows the previous one, and that `P9999Y` does not expand it.
- [x] 3.2 Preview reports truncation at `MAX_LOOPED_INTERVALS`. Verify: Rust test with a 15-minute forever pattern over 30 days.

- [x] 3.3 `GET /api/signals/registry`: for each payload type the lab reads, `{payloadType, units, currency}` derived from `lab_core::wire_contract::event_quantity_of` + `Quantity::units/currency` (no new table). Failing Rust test first. Verify: `cargo test -p vtn-bff registry` green; the test asserts `PRICE` → `KWH`/`EUR`.

## 4. UI

- [x] 4.1 `TariffGrid` component: cells, paste, fill-down, set-selection, invalid marking. Verify: component test pastes 24 lines and finds 24 filled rows.
- [x] 4.2 Recurrence control with period label ("repeats every 24 h"), truncated notice and DST note. Verify: component tests for the four recurrence states and a window across 2026-10-25.
- [x] 4.3 `FleetTariffChart` optional `draftBands` dashed series; page `Tariff.tsx` with live/draft overlay, dissent warning, targeting line, and fleet power chart on the same window; route and nav entry in `App.tsx`. Verify: Vitest for the page; `npm run build` and `eslint` clean.
- [ ] 4.4 (OPEN after merge: the wording follows ui-transparency and is covered by `Tariff.test.tsx`, but the screenshot needs a stack running the new BFF; take it after the next VTN deploy, then delete this change) Layout and wording follow ui-transparency: draft vs live legend, "published at" status, polling hint. Verify: screenshot of the page against the running stack.

## 5. Publish

- [x] 5.1 Publish dialog showing target VENs, new priority and the BL-02 tie note; uses the existing create-event hook. Verify: test asserts the POST body.
- [x] 5.2 Supersede behaviour: old event untouched, new event priority computed by D6. Verify: unit test for priority arithmetic including the floor at 0.

## 6. Use case, BDD, docs

- [x] 6.1 BDD scenario in `tests/features/` "change the tariff and the fleet plan follows" (publish a draft with a `forever` pattern, then assert VEN tariff snapshot and plan change). Verify: `python ci/audits/audit_step_definitions.py` clean, scenario runs green on Node2 under `node2-lock`.
- [x] 6.2 Update `docs/use-cases/` (tariff editing, recurrence), `docs/architecture/VTN_ARCHITECTURE.md` (preview endpoint, registry), project_journal and KEY_LEARNINGS entries. Verify: files changed in the same commit.
- [x] 6.3 Run the audits (`audit_duplication.py`, `audit_debt_gate.py`, file sizes) and eslint; then run the suites on Node2 by hand (`DOCKER_HOST=Node2 bash run_all_tests.sh`, under `node2-lock`) and merge the traditional way. Do NOT use `ci-handoff.sh`: another session is repairing the CI runner and needs its queue empty. Verify: audits exit 0 and the Node2 suite is green.
