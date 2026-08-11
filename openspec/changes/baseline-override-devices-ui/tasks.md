## 1. Unit test first (test-first)

- [ ] 1.1 Write `VEN/ui/src/__tests__/BaselineOverrideCard.test.tsx` against a not-yet-existing
      `BaselineOverrideCard`, asserting: empty state renders when `useBaselineOverride` returns `null`;
      adding a row + Save calls `postBaselineOverride` with a `slots` array matching the edited
      `slot_start`/`add_kw` rows; Clear calls `deleteBaselineOverride` when an override is active; Save
      is disabled with zero rows; Clear is disabled with no active override. Mock the three hooks the
      same way existing Devices-card tests mock their hooks.
- [ ] 1.2 Run `cd VEN/ui && npm test -- BaselineOverrideCard` and confirm it fails (component doesn't
      exist yet).

## 2. BaselineOverrideCard component

- [ ] 2.1 Create `VEN/ui/src/components/devices/BaselineOverrideCard.tsx` following the
      `ComfortCurveCard.tsx` structure: `useBaselineOverride()` for read, local `edited` overlay state
      (`BaselineSlot[] | null`), rows derived as `edited ?? data?.slots ?? []`.
- [ ] 2.2 Implement per-row editor: `slot_start` as a `datetime-local` input converted to/from ISO 8601
      at the component boundary, `add_kw` as a `type="number"` input (unit-suffixed per naming
      convention, already correct in the wire type).
- [ ] 2.3 Implement add-row and remove-row actions on local state only (no network call).
- [ ] 2.4 Implement Save: `usePostBaselineOverride().mutateAsync({ slots: rows })`, disabled while
      pending or `rows.length === 0`, clears local `edited` overlay on success.
- [ ] 2.5 Implement Clear: `useDeleteBaselineOverride().mutateAsync()`, disabled while pending or when
      `!data` (no active override), clears local `edited` overlay on success.
- [ ] 2.6 Implement empty state ("No baseline override active") when `data` is null/undefined, and an
      `updated_at` caption/chip when an override is active.
- [ ] 2.7 Add `data-testid`s following the `comfort-*` naming convention: `baseline-override-card`,
      `baseline-row-{i}`, `baseline-slot-start-{i}`, `baseline-add-kw-{i}`, `baseline-remove-{i}`,
      `baseline-add-btn`, `baseline-save-btn`, `baseline-clear-btn`.
- [ ] 2.8 Re-run `cd VEN/ui && npm test -- BaselineOverrideCard` and confirm it passes.

## 3. Mount on the Devices page

- [ ] 3.1 Import and render `BaselineOverrideCard` in `VEN/ui/src/pages/Devices.tsx` inside the existing
      `Grid container`, alongside `EvCard`/`HeaterCard`/`ShiftableLoadsCard`/`ComfortCurveCard`/
      `ArbiterSettingsCard` — no new props needed from `DevicesPage` itself since the card owns its own
      hooks, matching `ComfortCurveCard`'s self-contained pattern.
- [ ] 3.2 Run the full Devices page test suite (`cd VEN/ui && npm test`) to confirm no regressions.

## 4. BDD coverage

- [ ] 4.1 Write `tests/features/ven_ui_devices.feature` (new file, `@ven-ui` tag) with a background
      ("Given the VEN UI is open") and the end-to-end scenario from
      `specs/baseline-override-ui/spec.md` ("End-to-end baseline override set and clear via the UI"),
      using the existing testid-driven step vocabulary from `tests/features/ven_ui_planner.feature`.
- [ ] 4.2 Check whether existing generic step definitions (navigate to page by testid, click by testid,
      fill field by testid, assert element visible by testid) cover every step needed; add new step
      definitions only for genuinely new interactions not already covered (e.g. filling a specific
      datetime-local input) — confirm before writing new Python step code.
- [ ] 4.3 Run the scenario (`bash run_all_tests.sh --e2e` on Node1, per node1-lock / test-host-preference
      rules — prefer Node2 if available) and confirm it fails before the UI change, passes after.

## 5. Documentation

- [ ] 5.1 Update `docs/use-cases/HEMS-USE-CASE-OBSERVATION-MANUAL.md`'s Devices page card list (line
      ~20) to include "Baseline Override" alongside the existing card names.

## 6. Verification (BL-42 acceptance)

- [ ] 6.1 Confirm the new unit test (`BaselineOverrideCard.test.tsx`) passes and asserts calls into
      `postBaselineOverride`/`deleteBaselineOverride`.
- [ ] 6.2 Manually verify (or via the BDD scenario) that setting a baseline override changes the next
      plan's baseline-load input, e.g. by observing a `plan` query invalidation / updated plan values
      after Save.
- [ ] 6.3 Run `knip` (or the project's configured unused-export check) and confirm
      `useBaselineOverride`/`usePostBaselineOverride`/`useDeleteBaselineOverride` are no longer reported
      as unused.
- [ ] 6.4 Run `cd VEN/ui && npm run build` and eslint to confirm zero errors.

## 7. Close out

- [ ] 7.1 Once all tasks above are done and tests are green, delete this openspec change directory per
      the project's no-lingering-plans workflow (after waving any durable notes into
      `docs/reference/KEY_LEARNINGS.md` if applicable — none currently anticipated, this is a
      straightforward wiring change).
