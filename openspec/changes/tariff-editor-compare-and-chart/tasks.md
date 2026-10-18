# Tasks

## 1. Search

- [ ] 1.1 Find where fleet import/export power history can be read for a past window (history store, BFF fleet routes) and record the choice for D1 in design.md. Verify: inventory row updated, no "to be confirmed" left.

## 2. Operators (test first)

- [ ] 2.1 Failing tests then `signalOperators.ts`: scale, shift (wrapping within the pattern), cap, add-range, templates; recurrence unchanged. Verify: `cd VTN/ui && npm test -- signalOperators` green.
- [ ] 2.2 Toolbar rendered from the operator table. Verify: component test lists every table entry.

## 3. Chart editing

- [ ] 3.1 Pixel↔(time,value) mapping with snapping, as a pure function. Verify: unit tests for snap examples from the spec.
- [ ] 3.2 SVG overlay with drag handles on `FleetTariffChart`; edits go through draft functions; a drag on a repeated day edits the pattern. Verify: component test drags a step on day 2 and finds day 1 changed and 24 draft intervals.
- [ ] 3.3 Readout while dragging; grid reflects the change. Verify: test asserts the grid cell value.

## 4. Comparison

- [ ] 4.1 Reference series from the source chosen in 1.1, keyed by event id, recoverable after reload. Verify: test reloads the page state and finds the same reference.
- [ ] 4.2 Delta (import/export energy and peak) and "n of m VENs replanned" status. Verify: unit tests for the delta and for the provisional label.
- [ ] 4.3 Comparison panel on `Tariff.tsx` with legend (reference/live). Verify: Vitest, eslint and `npm run build` clean; screenshot against the running stack.

## 5. Use case and docs

- [ ] 5.1 Extend the BDD scenario from `tariff-editor-workbench` with the before/after check. Verify: `python ci/audits/audit_step_definitions.py` clean; scenario green on Node2 under `node2-lock`.
- [ ] 5.2 Update `docs/use-cases/`, `docs/architecture/VTN_ARCHITECTURE.md`, journal and KEY_LEARNINGS. Verify: files changed in the same commit.
