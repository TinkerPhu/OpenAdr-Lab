## 1. Expose the shared data builder

- [x] 1.1 Export `buildStackedFromAllTimelines` from `GridAccumulatedCell.tsx` (or relocate
      it to a shared module — decide during implementation per design.md's open question;
      default to exporting in place for two call sites) — already exported from prior work;
      no change needed
- [x] 1.2 No behavior change to `GridAccumulatedCell.tsx` itself; its existing test suite
      passes unchanged

## 2. Rewire PlanPowerStack to the timeline data source

- [x] 2.1 Replace `PlanPowerStack.tsx`'s `usePlan()`-driven `buildStackedFromPlan()` call
      with `useAllTimelines(hoursBack: 0, hoursForward)` + `buildStackedFromAllTimelines()`
- [x] 2.2 Compute `hoursForward` from `usePlan()`'s plan horizon (`plan.slots` last `end`),
      same derivation `buildStackedFromPlan`'s caller used before — `usePlan()` stays for
      this, plus `PlanHeaderBar`/`PlanDecisionMatrix`/`SessionProgressBoard` elsewhere on
      the page (unaffected)
- [x] 2.3 Keep the PV-curtailment banner computation as-is (reading `usePlan()`'s
      `pv_forecast_kw`/`pv_used_kw` per slot) — left unchanged, per design.md's leaning
- [x] 2.4 Delete `buildStackedFromPlan()` and any now-unused helper code in
      `PlanPowerStack.tsx`; also derive `assetIds` from presence in the timeline response
      (`RENDER_ORDER.filter(id => allTimelines[id]?.length > 0)`) instead of from
      `plan.slots[*].planned_kw_by_asset`, since that field is no longer read

## 3. Tests

- [x] 3.1 Add a regression fixture: a plan slot with `net_import_kw ≈ 0`,
      `net_export_kw > 0` → assert the rendered `gridPowerKw` for that point is negative
      (not near-zero) — the exact shape of the bug being fixed. Confirmed red against the
      pre-fix implementation before implementing the fix (`__tests__/PlanPowerStack.test.tsx`)
- [x] 3.2 Update `__tests__/PlannerPage.test.tsx`'s `../api/hooks` mock to include
      `useAllTimelines`; added `__tests__/PlanPowerStack.test.tsx` exercising
      `buildStackedFromAllTimelines` output directly (mirroring
      `GridAccumulatedCell.test.tsx`'s pattern) instead of the deleted `buildStackedFromPlan`
- [x] 3.3 Confirm the PV-curtailment banner test(s) still pass unchanged — covered in
      `PlanPowerStack.test.tsx`
- [x] 3.4 `cd VEN/ui && npm test` — full suite green (520/521; the 1 failure,
      `pv_irradiance_one_shot.test.ts`, is a pre-existing failure unrelated to this change —
      it makes a live HTTP call to real VEN hardware and gets a 403 — confirmed present on
      `main` before this change via `git stash`)

## 4. Verification

- [x] 4.1 `cd VEN/ui && npm run build`; ESLint zero errors (0 errors, 9 pre-existing
      warnings unrelated to touched files)
- [ ] 4.2 Manual check in a running dev server with `min_import` (autarky) objective and
      active PV surplus: Planner tab's grid line goes negative during export slots and
      visually matches the Controller tab's grid line for the same time range
- [ ] 4.3 Manual check with `min_cost`/other objectives: no regression in import-heavy
      slots (grid line still positive/correct)
- [x] 4.4 `scripts/audit_file_sizes.py` — passed
