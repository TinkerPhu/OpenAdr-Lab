# Tasks

Order: sections 2 and 3 (UI only) first; section 4 after `fleet-plan-feed` is merged.

## 1. Search

- [x] 1.1 Find where fleet import/export power history can be read for a past window. Result (2026-10-18): only actual power (`/api/fleet/power`) and per-event reactions exist; the VEN publishes no plan to the VTN. Resolved by the new change `fleet-plan-feed`; D1 and the inventory in design.md are updated. Verify: no "to be confirmed" left in design.md.

## 2. Operators (test first)

- [ ] 2.1 Failing tests then `signalOperators.ts`: scale, shift (wrapping within the pattern), cap, add-range, templates; recurrence unchanged. Verify: `cd VTN/ui && npm test -- signalOperators` green.
- [ ] 2.2 Toolbar rendered from the operator table on the Tariff page. Verify: component test lists every table entry and applies one.

## 3. Chart editing

- [ ] 3.1 Pixel to (time, value) mapping with snapping, as a pure function. Verify: unit tests for the spec's snap example.
- [ ] 3.2 SVG overlay with drag handles on `FleetTariffChart`; edits go through draft functions; a drag on a repeated day edits the pattern. Verify: component test drags a step on day 2 and finds day 1 changed and 24 draft intervals.
- [ ] 3.3 Readout while dragging; the grid reflects the change. Verify: test asserts the grid cell value.

## 4. Comparison (needs `fleet-plan-feed`)

- [ ] 4.1 `utils/planDelta.ts`: `seriesStats` (energy kWh over the horizon, peak kW) and the per-direction delta; replan progress from the contributing plans' `created_at` against the publish instant. Verify: unit tests for energy, peak, delta and the "3 of 5" label.
- [ ] 4.2 Hooks reading `GET /api/fleet/plan?at=` for the reference (publish - 1 s) and now; the publish instant from the event's `createdDateTime`. Verify: hook tests with mocked client.
- [ ] 4.3 Comparison panel on `Tariff.tsx`: reference faint, result solid, delta per direction, provisional label, missing-feed message. Verify: Vitest, eslint, `npm run build` clean.

## 5. Use case and docs

- [ ] 5.1 Extend `tests/features/tariff_editor.feature`: publish a draft, wait for the VENs' replan, and assert the plan-feed answer for `at=now` differs from `at=publish - 1 s` where the price moved. Verify: `python ci/audits/audit_step_definitions.py` clean; green on Node1/2 under the lock.
- [ ] 5.2 Update `docs/use-cases/`, `docs/architecture/VTN_ARCHITECTURE.md`, journal and KEY_LEARNINGS. Verify: files changed in the same commit.
