# Feature Specification: VEN Timeline UI

**Feature Branch**: `005-ven-timeline-ui`
**Created**: 2026-03-16
**Status**: Draft
**Input**: User description: "specify the requirements in docs\plans\archive\spec_kit_call_3_ven-timeline-ui.md starting from line 17"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Backend Timeline Endpoints (Priority: P1)

A developer or operator calling the VEN API receives unified, merged past-and-future timeline data for any configured asset or for all assets in a single request. The timeline combines measured history from the asset history buffer with planned future power slots and tariff-derived cost/CO₂ data.

**Why this priority**: All UI visualisation and BDD test updates depend on having correct backend data. Without working endpoints, no UI work can proceed. This is the load-bearing foundation of the speckit.

**Independent Test**: Can be tested entirely via HTTP calls — `GET /timeline/ev`, `GET /timeline/grid`, `GET /timeline/all` — asserting response shape, sort order, and presence of both past and future points. No UI needed.

**Acceptance Scenarios**:

1. **Given** a VEN with EV history data and an active plan, **When** `GET /timeline/ev?hours_back=1&hours_forward=1` is called, **Then** the response is a JSON array of `AssetTimelinePoint` objects sorted by `ts`, containing both past (from history buffer) and future (from plan) entries.
2. **Given** any configured VEN, **When** `GET /timeline/all` is called, **Then** the response contains entries for every configured asset plus `"grid"`.
3. **Given** a VEN with tariff data, **When** `GET /timeline/grid` is called, **Then** the response includes tariff intervals, capacity limits, and net power data merged into timeline points.
4. **Given** a request with `hours_back=0&hours_forward=24`, **When** `GET /timeline/ev` is called, **Then** only future points within 24 hours are returned (no past points).
5. **Given** a future plan slot, **When** its timeline point is examined, **Then** it carries `cost_rate_eur_h` and `co2_rate_g_h` values derived from the applicable tariff at that slot's start time.

---

### User Story 2 - Asset Cell Diagram Fixes (Priority: P2)

An operator viewing the VEN controller dashboard sees correct, unbroken asset timeline charts for all assets including battery and base_load (previously missing), PV (previously showing wrong data), and has a stable X-axis with the NOW reference line always visible at the centre.

**Why this priority**: The diagram bugs are the most visible user-facing defects. Once the backend endpoints exist, fixing the charts is the highest-value UI work and validates the end-to-end data pipeline.

**Independent Test**: Can be tested by opening the controller V2 UI and visually confirming all asset cells show data, plus automated BDD scenarios asserting recharts reference line presence and data in previously-empty cells.

**Acceptance Scenarios**:

1. **Given** the controller V2 UI is open, **When** the EV asset cell is displayed, **Then** the timeline chart shows solid past charging power, dashed cost rate, and dotted CO₂ rate — with no gap in the past section.
2. **Given** the controller V2 UI is open, **When** the battery and base_load asset cells are displayed, **Then** both show power data in the past section (previously missing).
3. **Given** the controller V2 UI is open, **When** the PV asset cell is displayed, **Then** the chart shows actual generated power, not the export limit setpoint.
4. **Given** the controller V2 UI is open, **When** any asset cell chart is rendered, **Then** the X-axis spans ±1 hour from now and the NOW reference line (red dotted) is always visible at the centre.
5. **Given** the controller V2 UI is open, **When** any asset cell chart is rendered, **Then** `AssetTimePoint.isPast` does not appear in the data — past vs. future is implicit in timestamp position.

---

### User Story 3 - Per-Cell Extended Time Window Toggle (Priority: P3)

An operator can toggle an extended time window on individual asset cells to see further into the future (e.g., full EV charging horizon, 24h tariff forecast) without affecting other cells.

**Why this priority**: Builds on US2 chart fixes. Each cell can independently request a wider time window from the backend. Provides operational value for planning decisions.

**Independent Test**: Can be tested by clicking the toggle icon on the EV cell and confirming the chart expands to show 24h forward, while other cells remain at ±1h.

**Acceptance Scenarios**:

1. **Given** the EV asset cell is in default view, **When** the extended window toggle is activated, **Then** the chart switches to `hours_forward=24` and shows the full projected charging horizon.
2. **Given** the Tariff/GridTariffCell is in default view, **When** the extended window toggle is activated, **Then** the chart shows `hours_back=0, hours_forward=24` (no past, 24h tariff forecast).
3. **Given** the Heater, PV, or BaseLoad cell, **When** the toggle icon is checked, **Then** no extended window is available for these asset types.
4. **Given** the EV cell has the extended window active, **When** the toggle is deactivated, **Then** the chart returns to the ±1h default window.

---

### User Story 4 - Schema-Driven Simulation Controls (Priority: P4)

An operator interacts with simulation controls in the asset cell right section. Controls are rendered dynamically from the backend control schema rather than hardcoded per-asset-type logic, enabling new asset types to appear without frontend code changes.

**Why this priority**: Decouples UI from backend asset type knowledge. Lower priority than chart data accuracy but important for maintainability.

**Independent Test**: Can be tested by verifying `GET /sim/schema` returns descriptors and the UI renders the correct control type (Slider, Switch, NumberInput) for each descriptor.

**Acceptance Scenarios**:

1. **Given** a VEN with an EV asset, **When** the EV cell right section is rendered, **Then** controls (e.g., plugged toggle, SoC slider) are generated from the schema descriptors — not from hardcoded asset-type conditionals.
2. **Given** `GET /sim/schema` returns a new control descriptor for an asset, **When** the UI renders that asset's right section, **Then** the correct control type is shown without any frontend code change.
3. **Given** an override is changed via a dynamic control, **When** the POST body is sent, **Then** it uses the generic `Record<string, value>` format matching the schema key names.

---

### User Story 5 - GridAccumulatedCell Stacked Area from Backend (Priority: P5)

The GridAccumulatedCell stacked area chart renders power contributions per asset using data from the `GET /timeline/all` endpoint, replacing the frontend `buildStackedAreaData` assembly function.

**Why this priority**: Completes the removal of all frontend data assembly, achieving a clean backend-driven data pipeline. Depends on US1 and US2 being complete.

**Independent Test**: Can be tested by confirming the stacked area chart renders with per-asset area series and that `buildStackedAreaData` is no longer called.

**Acceptance Scenarios**:

1. **Given** the controller V2 UI is open, **When** the GridAccumulatedCell is displayed, **Then** it renders a stacked area chart with per-asset `power_kw` series from `useAllTimelines`.
2. **Given** all timeline data is from the backend, **When** the chart renders, **Then** positive values stack above the X-axis and negative values (export) stack below.

---

### User Story 6 - API Rename and Codebase Cleanup (Priority: P6)

All TypeScript source uses `useTariffs` / `TariffSnapshot` consistently (renaming from `useRates` / `RateSnapshot`). Frontend data assembly functions `buildAssetTimeline`, `buildTariffTimeline`, `buildStackedAreaData` are deleted. The `nowMs` memoization bug is fixed.

**Why this priority**: Hygiene and correctness. Fixes the naming inconsistency introduced when the backend renamed `/rates` → `/tariffs`. The `nowMs` bug causes chart instability. Can be done alongside other work.

**Independent Test**: Verifiable by searching the TypeScript source — `useRates` and `RateSnapshot` must not appear anywhere. `buildAssetTimeline`, `buildTariffTimeline`, `buildStackedAreaData` must not exist.

**Acceptance Scenarios**:

1. **Given** any TypeScript source file, **When** searching for `useRates` or `RateSnapshot`, **Then** no results are found.
2. **Given** `dataBuilders.ts`, **When** its contents are checked, **Then** `buildAssetTimeline`, `buildTariffTimeline`, and `buildStackedAreaData` are absent.
3. **Given** the controller V2 page component, **When** `nowMs` is used, **Then** it is computed with `useMemo` (computed once per mount, not every render).

---

### Edge Cases

- What happens when an asset has no history (newly started VEN)? Timeline endpoint returns only future plan points (or empty array if no plan either).
- What happens when the plan is empty? Future section of timeline is empty; past section still renders from history buffer.
- What happens when `hours_back=0` and `hours_forward=0`? Returns an empty array.
- What happens when an `asset_id` is not recognised and is not `"grid"`? Endpoint returns 404.
- What happens when timestamps from history and plan overlap near `now`? Merge is sorted by `ts`; both entries are included (no deduplication).
- How does the chart behave when `useAllTimelines` is loading? Chart must not crash on `undefined`/`null` data; show empty or cached state gracefully.

## Requirements *(mandatory)*

### Functional Requirements

**Backend**

- **FR-001**: The system MUST expose `GET /timeline/{asset_id}` returning a JSON array of `AssetTimelinePoint` sorted by `ts`, merging past history and future plan data for the named asset.
- **FR-002**: The system MUST expose `GET /timeline/all` returning a map of asset ID to `Vec<AssetTimelinePoint>`, covering every configured asset plus `"grid"`.
- **FR-003**: `GET /timeline/grid` MUST be supported as a valid `asset_id` in `GET /timeline/{asset_id}`, returning tariff intervals, capacity limits, and net power timeline.
- **FR-004**: Both timeline endpoints MUST accept `hours_back` and `hours_forward` query parameters defaulting to 1.0 each, and return only points within the requested window.
- **FR-005**: Future timeline points MUST carry `cost_rate_eur_h` and `co2_rate_g_h` values computed from the rate schedule applicable at the slot's start time.
- **FR-006**: Past timeline points MUST carry `cost_rate_eur_h` and `co2_rate_g_h` values as stored in `AssetHistoryBuffer`.
- **FR-007**: The `build_asset_timeline` function MUST be side-effect-free and fully unit-testable.
- **FR-008**: `GET /sim/schema` MUST return the control descriptor map for all configured assets.

**Frontend — API Layer**

- **FR-009**: The UI MUST add `useTimeline(assetId, hoursBack, hoursForward)`, `useAllTimelines(hoursBack, hoursForward)`, and `useSimSchema()` hooks.
- **FR-010**: The UI MUST remove any hook that previously joined `/trace` + `/plan` + `/rates` data in the frontend.
- **FR-011**: The UI MUST rename `useRates` → `useTariffs` and `RateSnapshot` → `TariffSnapshot` throughout all TypeScript source.

**Frontend — Data Assembly Removal**

- **FR-012**: `buildAssetTimeline`, `buildTariffTimeline`, and `buildStackedAreaData` MUST be deleted from `dataBuilders.ts`.
- **FR-013**: `findCurrentTariff`, `deriveAssetSummaries`, `deriveTariffSnapshot` MAY be retained only if still used for live sim snapshot summary stats; otherwise deleted.

**Frontend — Chart Fixes**

- **FR-014**: All asset cells (including battery and base_load) MUST show past power data sourced from the timeline endpoint.
- **FR-015**: The PV cell MUST display actual generated power (`power_kw` from history), not the reactor export limit setpoint.
- **FR-016**: The X-axis domain MUST be fixed at ±1h by default; the NOW reference line MUST always be visible at the centre.
- **FR-017**: `AssetTimePoint.isPast` MUST be removed from all TypeScript types and data flows.
- **FR-018**: Line styles MUST be: power = solid, cost rate = dashed, CO₂eq = dotted — applied consistently across the full time span.
- **FR-019**: `nowMs` MUST be computed with `useMemo` (once per page mount) rather than recalculated every render.

**Frontend — Per-Cell Extended Window**

- **FR-020**: Each asset cell header MUST include an icon toggle button for extended time window mode; state is per-cell and session-scoped.
- **FR-021**: Extended window parameters per cell type MUST be: EV → `hours_forward=24`; Battery → `hours_forward=24`; Tariff/GridTariffCell → `hours_back=0, hours_forward=24`; Heater/PV/BaseLoad → no extended view.
- **FR-022**: The toggle MUST update `hoursBack`/`hoursForward` parameters passed to the hook; the backend returns the exact requested window with no frontend trimming.

**Frontend — Dynamic Controls**

- **FR-023**: `AssetRightSection` MUST render controls from `useSimSchema()` descriptors with no hardcoded per-asset-type logic.
- **FR-024**: `DynamicControl` MUST render a Slider, Switch, or NumberInput based on `descriptor.kind`.
- **FR-025**: Override POST body MUST be a generic `Record<string, value>` keyed by schema descriptor key names.

**Frontend — GridAccumulatedCell**

- **FR-026**: `GridAccumulatedCell` MUST use `useAllTimelines` as its data source for the stacked area chart.
- **FR-027**: The stacked area MUST split positive (import) and negative (export) values per asset, stacking correctly on both sides of the X-axis.

**BDD Tests**

- **FR-028**: BDD scenarios for `GET /sim` response assertions MUST be updated to use `assets.<id>.values.<key>` field paths.
- **FR-029**: New BDD scenarios MUST cover: `GET /timeline/{asset_id}` merged points; `GET /timeline/grid`; `GET /timeline/all`; per-cell extended window horizon.
- **FR-030**: Existing UC-01–UC-12 controller scenarios MUST remain passing with no changes.

### Key Entities

- **AssetTimelinePoint**: Single time-stamped data point for one asset. Fields: `ts` (epoch ms / `DateTime<Utc>`), `values` (map of well-known key strings to f64). No `is_forecast` field.
- **TimeWindow**: Request parameter pair `{ hours_back, hours_forward }` controlling the timeline slice returned.
- **ControlDescriptor**: Schema entry describing a simulation override control. Includes `key`, `kind` (Slider | Switch | NumberInput), bounds, and labels. Sourced from `GET /sim/schema`.
- **TariffSnapshot** (renamed from RateSnapshot): Time-bounded tariff record with import/export price per kWh and CO₂ intensity. Used to enrich future timeline points.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: `GET /timeline/ev?hours_back=1&hours_forward=1` returns a sorted array with at least one past point and one future point when EV has history and an active plan.
- **SC-002**: `GET /timeline/all` returns entries for all configured assets and `"grid"` in a single response with no missing assets.
- **SC-003**: All asset cells in the controller V2 UI display timeline charts with continuous data — no cell is empty or shows a gap in the past section.
- **SC-004**: The PV cell chart shows values consistent with the PV physics model (0 at night, positive 6am–6pm), not the reactor setpoint limit.
- **SC-005**: The NOW reference line is visible on every asset cell timeline chart, positioned at the centre of the ±1h default window.
- **SC-006**: Activating the extended window toggle on the EV cell causes the chart to show at least 4h of future data; other cells remain at ±1h.
- **SC-007**: Zero occurrences of `useRates`, `RateSnapshot`, `buildAssetTimeline`, `buildTariffTimeline`, `buildStackedAreaData`, or `AssetTimePoint.isPast` in TypeScript source.
- **SC-008**: `AssetRightSection` renders the correct number of controls matching `GET /sim/schema` descriptor count for each asset, with no console errors.
- **SC-009**: All existing BDD scenarios (27+ features, 123+ scenarios) continue to pass after the data pipeline change.
- **SC-010**: All new BDD scenarios for timeline endpoints (per-asset, grid, all-assets, extended window) pass.

## Assumptions

- `AssetHistoryBuffer` is already being written per-tick by `monitor.rs` (delivered in speckit 004). The `cost_rate_eur_h` and `co2_rate_g_h` fields are stored in each history row.
- `GET /trace/history` returns raw history buffer rows; the new `GET /timeline/{asset_id}` endpoint is a separate, enriched view that merges plan future data.
- The plan's net import/export fields are accessible for building the grid timeline.
- `GET /sim/schema` may already be wired from speckit 001; this speckit adds the first UI consumer. If not yet wired, it is in scope.
- The backend rename `/rates` → `/tariffs` was completed in speckit 004. This speckit completes only the frontend rename.
- All assets are sampled on the same 1-second tick clock, so timestamps in `GET /timeline/all` align exactly — no interpolation needed for stacking.
- Session-scoped per-cell extended window state does not need to survive page reload.
