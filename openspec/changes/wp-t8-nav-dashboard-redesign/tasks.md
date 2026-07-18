## 1. Nav regrouping

- [x] 1.1 Reorder `App.tsx` primary nav to Dashboard, Devices, Controller,
      History, Planner, Notifications (§3.2 order); keep existing testids.
- [x] 1.2 Add a "VTN Feed" `Menu` (button `nav-vtn-feed-menu`) containing
      Reports/Programs/Events `MenuItem`s, testids unchanged
      (`nav-reports`/`nav-programs`/`nav-events`).
- [x] 1.3 Add a "Diagnostics" `Menu` (button `nav-diagnostics-menu`)
      containing Metrics/Raw Data/Tasks/Event Log `MenuItem`s, testids
      unchanged.
- [x] 1.4 Update `App.test.tsx`: primary tabs visible directly; VTN
      Feed/Diagnostics items only visible after opening their menu.

## 2. Dashboard status rows

- [x] 2.1 Add `useVtnStatus` hook to `hooks.ts` (wraps the existing but
      unused `client.vtnStatus()`).
- [x] 2.2 New `VEN/ui/src/components/dashboard/StatusRows.tsx`:
      `VtnConnectionRow`, `PlanStatusRow`, `TaskSummaryRow` — each a single
      green line when healthy, `Collapse`-expanding detail when degraded
      (same idiom as `PlanHeaderBar.tsx`'s warnings expand).
- [x] 2.3 Wire the three rows into `Dashboard.tsx`, above the existing cards.
- [x] 2.4 Unit tests for each row: healthy single-line state, degraded
      expanded-detail state, and (Plan row only) the neutral no-plan-yet
      state.

## 3. UI suite gate

- [x] 3.1 `npx tsc --noEmit` clean.
- [x] 3.2 ESLint zero errors.
- [x] 3.3 `cd VEN/ui && npm test` — all pass.

## 4. Bookkeeping

- [x] 4.1 Mark WP-T8 as done in `docs/plans/ven-ui-transparency.md` §4/§7.
- [x] 4.2 Note completion in `docs/history/project_journal.md`.
