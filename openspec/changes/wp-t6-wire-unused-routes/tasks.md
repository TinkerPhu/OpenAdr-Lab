## 1. Types + client + hooks

- [x] 1.1 Added `PlanSnapshot`, `ReportObligation`, `AssetCapability`,
      `ForecastSource`, `AssetForecast` types to `api/types.ts` (field-for-field
      against the actual Rust structs, confirmed by reading them first, not
      guessed).
- [x] 1.2 Added `historyPlans`, `obligations`, `assetCapability`,
      `assetForecasts` client methods to `api/client.ts`.
- [x] 1.3 Added `useHistoryPlans`, `useObligations`, `useAssetCapabilities`
      (parallel `useQueries`, no bulk endpoint exists), `useAssetForecasts` to
      `api/hooks.ts`.

## 2. Controller page: Flexibility & Forecast panel

- [x] 2.1 New standalone `components/controller/FlexibilityForecastPanel.tsx` —
      deliberately not new `AssetCell` props (design.md D1: avoids touching an
      already-large, tightly-composed component for a WP whose only goal is
      surfacing existing data).
- [x] 2.2 Wired into `pages/Controller.tsx` above the grid/asset cells.
- [x] 2.3 Unit tests (`__tests__/FlexibilityForecastPanel.test.tsx`): empty
      state, dash fallback when data is missing, full render with both
      capability and forecast present, fixed-capability labeling.

## 3. History page: Plans section

- [x] 3.1 Added a "Plans" table (created/horizon start/horizon end/View) below
      the existing Reports-sent table in `pages/History.tsx`, reusing the
      existing `JsonDialog` component for plan detail (design.md D3).
- [x] 3.2 Unit tests (extended `__tests__/History.test.tsx`): renders a plan
      row from mocked data; clicking View opens the JSON dialog with the
      parsed plan content.

## 4. Reports page: Pending Obligations section

- [x] 4.1 Added a "Pending Obligations" table above the existing search/reports
      table in `pages/Reports.tsx`, with a client-computed Pending/Overdue/
      Fulfilled status (design.md D4 — no server-side `is_due` field exists on
      the wire shape).
- [x] 4.2 Unit tests (extended `__tests__/Reports.test.tsx`): a not-yet-due
      obligation renders "Pending"; a past-due, unfulfilled obligation renders
      "Overdue".

## 5. UI suite gate

- [x] 5.1 `npx tsc --noEmit` clean.
- [x] 5.2 ESLint zero errors — one true error caught and fixed along the way:
      `Reports.tsx` originally called `Date.now()` inline during render for the
      overdue check (`react-hooks/purity`); fixed by hoisting to a single
      `nowMs` computed once per render with the same
      `eslint-disable-next-line react-hooks/purity -- intentional` justification
      pattern already used elsewhere in this codebase (`Controller.tsx`).
- [x] 5.3 `cd VEN/ui && npm test` — 381/381 passed. Two pre-existing test files
      (`GridTariffCell.test.tsx`, `GridAccumulatedCell.test.tsx`) also render
      `ControllerPage` (hence `FlexibilityForecastPanel`) and needed their
      `../api/hooks` mocks extended with `useAssetCapabilities`/
      `useAssetForecasts` — caught by the first full suite run failing with 6
      failures, not assumed away.

## 6. Bookkeeping

- [x] 6.1 Mark WP-T6 as done in `docs/plans/ven-ui-transparency.md` §4/§7.
- [x] 6.2 Note in `docs/history/project_journal.md`: the two-forecast-concepts
      naming collision (`/forecast` vs. `/forecast/:asset_id`), the
      `FlexibilityForecastPanel`-not-`AssetCell` isolation decision, and the
      cross-test-file mock-gap lesson.
