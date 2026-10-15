# Tasks

Branch: `refactor/ui-per-asset`.

## 1. Roster order on the wire, one helper

- [x] 1.1 `roster_index` serialized on `AssetSnapshot`; generated types.
- [x] 1.2 Test first: `assetsOfType` orders by roster and filters by type.

## 2. Devices

- [x] 2.1 Test first: two EVs → two cards with their own switch; a card's request names its asset.
- [x] 2.2 EV and heater cards per asset; comfort-curve assets from the roster.

## 3. Controller, charts, dashboard

- [x] 3.1 Test first: two batteries → two Controller cells.
- [x] 3.2 Summaries, right section, tariff builders, chart order, Dashboard badge per asset.

## 4. Checks, docs, merge

- [ ] 4.1 UI tests, lint, build; Rust checks; suites on Node2. (End-to-end two-EV Devices BDD comes
      with W7, the first profile that may hold two EVs.)
- [ ] 4.2 Docs, journal, wiki; delete this change; merge.
