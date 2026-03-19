# Research: VEN Raw Data Diagnostics Page

**Feature**: 006-ven-raw-diagnostics
**Date**: 2026-03-18

---

## Decision 1: Page location and navigation

**Decision**: Add a new `RawDiagnostics.tsx` page at route `/raw-diagnostics`, with a nav button added to the existing top navigation bar in `App.tsx`.

**Rationale**: Every page (Trace, Metrics, etc.) lives at `VEN/ui/src/pages/`. The nav bar in `App.tsx` has precedent for diagnostics-style pages (Trace, Metrics). No structural changes to the existing component tree are needed.

**Alternatives considered**: Adding as a sub-tab within ControllerV2 — rejected because this page is endpoint-agnostic and belongs at the same level as Trace/Metrics.

---

## Decision 2: Data fetching pattern — manual refresh

**Decision**: Use TanStack React Query `useQuery` with `enabled: false` plus an explicit `refetch()` call wired to the refresh button. Each cell manages its own query independently.

**Rationale**: React Guidelines mandate TanStack React Query for all API access. `enabled: false` disables auto-fetching; `refetch()` is the standard React Query way to trigger manual fetches. This keeps error/loading states co-located with each cell and ensures one cell's fetch never triggers another.

**Alternatives considered**: `useState` + `useEffect` + direct `api.method()` — rejected as it bypasses the project's mandatory React Query pattern.

---

## Decision 3: VenApi methods — no new additions needed

**Decision**: All three endpoints are already exposed in `VenApi` (`client.ts`):
- `/sim` → `api.sim()` → `SimSnapshot`
- `/tariffs` → `api.rates()` (legacy name; holds tariff data per memory entry `feedback_tariff_rate_nomenclature.md`) → `PlannedRates` / `TariffSnapshot[]`
- `/timeline/all` → `api.allTimelines(params)` → `Record<string, AssetTimelinePoint[]>`

**Rationale**: VenApi already covers all needed endpoints. Adding wrapper hooks is unnecessary for a manual-refresh page.

**Alternatives considered**: Adding dedicated hooks in `hooks.ts` — not needed since React Query's `useQuery` can call api methods directly; hooks add boilerplate for no benefit here.

---

## Decision 4: Sim chart — categorical profile line

**Decision**: `/sim` returns a single snapshot (not time-series). Display it as a **categorical line chart** where x-axis = asset ID and y-axis = `power_kw` for that asset. Grid net power (`net_power_w`) is included as an additional point labeled "grid". Points are connected by a line in a single color.

**Rationale**: The user wants "data points connected by lines." A snapshot has multiple named asset readings — treating asset IDs as categories and connecting them with a line creates a meaningful "power profile" view. This is the only sensible line-chart interpretation for snapshot data.

**Alternatives considered**:
- Bar chart — visually clearer for categorical data, but user specified lines.
- Table/JSON view — already covered by `JsonDialog`; doesn't add value here.

---

## Decision 5: Tariffs chart — interval-based multi-line

**Decision**: Display `TariffSnapshot[]` as a multi-line time-series chart. X-axis = `interval_start`. Series: `import_price_eur_kwh`, `export_price_eur_kwh`, `co2_g_kwh`. Each series in a distinct color. `null` values are rendered as gaps.

**Rationale**: The three price dimensions are independent series — each gets its own line. Standard recharts `LineChart` handles `null` gaps naturally.

**Alternatives considered**: Using `is_forecast` to differentiate line style (dashed vs solid) — useful but adds complexity; out of scope for raw view.

---

## Decision 6: Timeline chart — dynamic series dropdown

**Decision**: Display one series at a time from `/timeline/all`, selected via a dropdown. Available options are derived from the response keys at fetch time (e.g., `ev`, `battery`, `heater`, `pv`, `base_load`, `grid`). Default selection is `grid`. X-axis = `ts` (epoch ms). Y-axis = `power_kw` from the `values` map.

**Rationale**: Showing all assets simultaneously on one raw chart creates too many overlapping lines. The dropdown keeps the view focused while preserving access to all series. `power_kw` is the most universally available field across all asset types.

**Alternatives considered**: Show all series simultaneously with toggle checkboxes — useful but out of scope for initial raw view.

**Time window**: ±1 hour from now, passed as `hours_back=1.0&hours_forward=1.0`. Endpoint default is already 1 hour, so params can be omitted but are included explicitly for clarity.

---

## Decision 7: Component structure

**Decision**: Three levels of components:
1. `RawDiagnostics.tsx` (page) — renders four `DiagnosticCell` instances stacked vertically.
2. `DiagnosticCell.tsx` (reusable wrapper) — accepts a title, a `useQuery` result, a refresh handler, and a render prop (the chart). Handles loading/error states.
3. Three chart components in `components/raw-diagnostics/`: `SimProfileChart.tsx`, `TariffsLineChart.tsx`, `TimelineSeriesChart.tsx`.

**Rationale**: The cell wrapper encapsulates the loading/error/refresh button pattern once. Chart components are pure (receive already-fetched data as props) for easy testing.

**Alternatives considered**: Inlining all chart logic directly in the page — rejected as it creates a 500+ line page that is hard to test.

---

## Resolved Clarifications from Spec

| Clarification | Resolution |
|---|---|
| `/timeline/all` accepts time-range params | Yes — `hours_back` and `hours_forward` query params |
| Series list for Timeline dropdown | Derived from response keys at fetch time; default = "grid" |
| `/tariffs` endpoint name | Currently registered as `/rates` in the VenApi; plan uses that name unless renamed |
| No auto-refresh required | Confirmed — manual refresh only, `enabled: false` in useQuery |
