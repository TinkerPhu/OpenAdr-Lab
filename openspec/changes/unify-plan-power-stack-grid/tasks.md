## 1. Expose the shared data builder

- [ ] 1.1 Export `buildStackedFromAllTimelines` from `GridAccumulatedCell.tsx` (or relocate
      it to a shared module — decide during implementation per design.md's open question;
      default to exporting in place for two call sites)
- [ ] 1.2 No behavior change to `GridAccumulatedCell.tsx` itself; its existing test suite
      passes unchanged

## 2. Rewire PlanPowerStack to the timeline data source

- [ ] 2.1 Replace `PlanPowerStack.tsx`'s `usePlan()`-driven `buildStackedFromPlan()` call
      with `useAllTimelines(hoursBack: 0, hoursForward)` + `buildStackedFromAllTimelines()`
- [ ] 2.2 Compute `hoursForward` from `usePlan()`'s plan horizon (`plan.slots` last `end`),
      same derivation `buildStackedFromPlan`'s caller used before — `usePlan()` stays for
      this, plus `PlanHeaderBar`/`PlanDecisionMatrix`/`SessionProgressBoard` elsewhere on
      the page (unaffected)
- [ ] 2.3 Keep the PV-curtailment banner computation as-is (reading `usePlan()`'s
      `pv_forecast_kw`/`pv_used_kw` per slot) unless implementation finds moving it to the
      timeline's PV series is a clean net simplification (design.md open question) — not
      required for this change
- [ ] 2.4 Delete `buildStackedFromPlan()` and any now-unused helper code in
      `PlanPowerStack.tsx`

## 3. Tests

- [ ] 3.1 Add a regression fixture: a plan slot with `net_import_kw ≈ 0`,
      `net_export_kw > 0` → assert the rendered `gridPowerKw` for that point is negative
      (not near-zero) — the exact shape of the bug being fixed
- [ ] 3.2 Update `__tests__/PlannerPage.test.tsx`'s `../api/hooks` mock to include
      `useAllTimelines`; update or add a `PlanPowerStack`-focused test exercising
      `buildStackedFromAllTimelines` output (mirroring `GridAccumulatedCell.test.tsx`'s
      pattern) instead of asserting on the deleted `buildStackedFromPlan`
- [ ] 3.3 Confirm the PV-curtailment banner test(s) still pass unchanged (or are updated if
      2.3 decides to move its data source)
- [ ] 3.4 `cd VEN/ui && npm test` — full suite green

## 4. Verification

- [ ] 4.1 `cd VEN/ui && npm run build`; ESLint zero errors
- [ ] 4.2 Manual check in a running dev server with `min_import` (autarky) objective and
      active PV surplus: Planner tab's grid line goes negative during export slots and
      visually matches the Controller tab's grid line for the same time range
- [ ] 4.3 Manual check with `min_cost`/other objectives: no regression in import-heavy
      slots (grid line still positive/correct)
- [ ] 4.4 `scripts/audit_file_sizes.py` — no touched file exceeds its limit
