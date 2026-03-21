# Feature Specification: Grid-Aligned UI Timeline

**Feature Branch**: `011-grid-aligned-ui`
**Created**: 2026-03-21
**Status**: Draft
**Input**: User description: "RF-05d: Remove findNearest, use grid-aligned API response"

## Clarifications

### Session 2026-03-21

- Q: Does the response shape change? -> A: No. The response format stays `Record<string, {ts, values | null}[]>`. RF-05c only adds alignment guarantees (same length, same `ts` at each index, now-point embedded in the array) and `values: null` for empty buckets. No structural change.
- Q: Which components are affected? -> A: All consumers of `allTimelines`: `GridAccumulatedCell` (stacked area), all `AssetCell` instances (per-asset charts via `AssetMidSection`), `GridTariffCell` (grid power line), `dataBuilders.ts` (`computeForecastEnergy`), `ControllerV2Page` (data distribution), and `RawDiagnostics` (direct API call). The `/tariffs` endpoint is NOT changed.
- Q: Does `/tariffs` change? -> A: No. Tariffs are sparse step functions (1-10 points per 24h), render correctly as-is. The tariff cell's tariff data parsing (`buildTariffPricePoints`) is unchanged. Only the `gridTimeline` prop (sourced from `allTimelines["grid"]`) benefits from alignment.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Accurate Stacked Area Chart (Priority: P1)

As a VEN operator viewing the Controller dashboard, I want the stacked area chart to use positional indexing across asset arrays so that chart values are accurate and free from interpolation artifacts.

**Why this priority**: The current tolerance-based nearest-neighbour matching (`findNearest` with `TOLERANCE_MS`) can produce incorrect power readings when downsampled timestamps drift between assets. Replacing it with positional zip is the core improvement.

**Independent Test**: Load the Controller V2 page and verify that stacked area chart data points are built by iterating a single shared index across all asset arrays, with no tolerance-based lookup.

**Acceptance Scenarios**:

1. **Given** the backend returns grid-aligned timeline data where all asset arrays share the same `ts` at each index, **When** the stacked area chart data is built, **Then** each row uses the same array index across all assets (positional zip).
2. **Given** a response with 5 known assets plus "grid", **When** the chart data builder runs, **Then** `gridPowerKw` at index N comes from `allTimelines["grid"][N].values.power_kw`.
3. **Given** an entry where `values` is `null` (empty grid bucket), **When** the chart data builder encounters it, **Then** that asset's contribution is treated as zero (or gap) at that timestamp.

---

### User Story 2 - All Asset Cell Charts Use Aligned Data (Priority: P1)

As a VEN operator, I want each individual asset cell chart to correctly handle `values: null` entries from empty grid buckets so that charts render without errors.

**Why this priority**: Every asset cell receives `allTimelines[assetId]` as its `timePoints` prop. The new response can contain `values: null` entries that the current code does not handle.

**Independent Test**: Render an asset cell with timeline data containing `values: null` entries and verify the chart renders without errors, showing gaps where data is missing.

**Acceptance Scenarios**:

1. **Given** an asset timeline containing entries with `values: null`, **When** the `AssetTimelineChart` renders, **Then** it treats null-values entries as gaps (no data point plotted) rather than crashing or showing zero.
2. **Given** `computeForecastEnergy` in `dataBuilders.ts` iterates future timeline points, **When** it encounters entries with `values: null`, **Then** it skips them (no energy contribution) rather than producing NaN or errors.

---

### User Story 3 - Grid Tariff Cell Handles Null Values (Priority: P1)

As a VEN operator, I want the tariff cell's grid power line to handle `values: null` entries from the grid asset's timeline so the chart does not crash.

**Why this priority**: `GridTariffCell` receives `allTimelines["grid"]` via its `gridTimeline` prop. The `buildPowerPoints` function in `tariffBuilders.ts` must handle null values.

**Independent Test**: Pass grid timeline data with `values: null` entries to `buildPowerPoints` and verify it produces null power/cost values at those timestamps.

**Acceptance Scenarios**:

1. **Given** a grid timeline with `values: null` entries, **When** `buildPowerPoints` processes them, **Then** it produces `TariffTimePoint` entries with `gridPowerKw: null` and `totalCostRateEurH: null` at those timestamps.

---

### User Story 4 - Clean Codebase (Priority: P2)

As a developer, I want `findNearest()`, `TOLERANCE_MS`, and the tolerance-based `buildStackedFromAllTimelines` logic removed since they are no longer needed.

**Why this priority**: Once positional indexing replaces nearest-neighbour lookup, this code is dead weight.

**Independent Test**: Search the codebase for `findNearest`, `TOLERANCE_MS`, and confirm zero results.

**Acceptance Scenarios**:

1. **Given** the positional-zip data builder is in place, **When** searching the codebase for `findNearest`, **Then** zero results are found.
2. **Given** the migration is complete, **When** searching for `TOLERANCE_MS`, **Then** zero results are found.

---

### User Story 5 - Resolution Query Parameter (Priority: P3)

As a developer, I want the API client to support the `resolution` query parameter (seconds) as the primary density control, with `max_points` kept as a deprecated alias.

**Why this priority**: Aligns the client with the RF-05c backend parameter change. Low risk since `max_points` is still accepted.

**Independent Test**: Call `api.allTimelines({ resolution: 30 })` and verify the request URL contains `resolution=30`.

**Acceptance Scenarios**:

1. **Given** the API client receives a `resolution` option, **When** building the request URL, **Then** it sends `?resolution=30` instead of `max_points`.
2. **Given** no `resolution` is specified and `maxPoints` is provided, **When** building the request, **Then** it sends the deprecated `max_points` parameter for backward compatibility.

---

### Edge Cases

- What happens when the backend returns an empty response (no assets, or all arrays empty)? Charts render with no data points and no errors.
- What happens when an asset is missing from the response (e.g., a VEN profile has no PV)? That asset's contribution is zero at every timestamp. The existing `?? []` fallback in `ControllerV2Page` handles this.
- What happens when ALL entries in an asset's array have `values: null`? The chart renders as a flat line at zero or an empty chart. No crash.
- What happens when `RawDiagnostics` calls `api.allTimelines()` directly (not via the hook)? It receives the same `Record<string, {ts, values|null}[]>` format. The raw diagnostics display should handle `null` values.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The API client's `allTimelines()` method MUST handle `values: null` entries in the response (new from RF-05c empty grid buckets). The existing return type `AssetTimelinePoint` MUST be updated so `values` can be `null`.
- **FR-002**: The `buildStackedFromAllTimelines` function MUST be replaced with a positional-zip implementation that iterates by shared index across all asset arrays instead of using nearest-neighbour search.
- **FR-003**: The positional-zip builder MUST handle `values: null` entries by treating the asset's contribution as zero at that timestamp.
- **FR-004**: The `findNearest()` function MUST be removed from `GridAccumulatedCell.tsx`.
- **FR-005**: The `TOLERANCE_MS` constant MUST be removed.
- **FR-006**: `computeForecastEnergy` in `dataBuilders.ts` MUST handle `values: null` entries by skipping them (no energy contribution).
- **FR-007**: `buildPowerPoints` in `tariffBuilders.ts` MUST handle `values: null` entries, producing null power/cost values.
- **FR-008**: The `AssetTimelineChart` (used by `AssetMidSection`) MUST render `values: null` entries as gaps or skip them, not crash or show zero.
- **FR-009**: The API client MUST support the `resolution` query parameter (seconds). The `maxPoints` parameter MUST be kept as a deprecated alias.
- **FR-010**: The `GridAccumulatedCell.test.tsx` and any affected test files MUST be updated to validate the positional-index approach and null-values handling.
- **FR-011**: `RawDiagnostics` page MUST handle `values: null` entries when displaying raw timeline data.

### Key Entities

- **AssetTimelinePoint** (updated): Existing type where `values` changes from `Record<string, number>` to `Record<string, number> | null` to accommodate empty grid buckets.
- **StackedAreaPoint**: Existing chart data type (unchanged) -- one row per timestamp with positive/negative split per asset plus grid power.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: The stacked area chart, all asset cell charts, and the tariff cell grid power line render correctly with grid-aligned data -- zero tolerance-based lookups remain.
- **SC-002**: All charts handle `values: null` entries without errors -- verified by unit tests with null-values fixtures.
- **SC-003**: All existing unit tests pass after migration, and new/updated tests validate the positional-index approach with at least 3 scenarios (normal data, empty/null data, mixed assets).
- **SC-004**: No regressions in the Controller V2 page -- asset power values, grid power overlay, tariff chart, forecast energy, and time-window filtering (1h / 24h) all work correctly.
- **SC-005**: `findNearest` and `TOLERANCE_MS` are fully removed -- zero references remain in the codebase.

## Assumptions

- RF-05c (backend uniform-grid timeline API) is implemented and deployed before this feature. The backend guarantees: same array length across all assets, same `ts` at each index, now-point embedded in the array, `values: null` for empty buckets.
- The `/tariffs` endpoint response is unchanged. Only the grid timeline prop in `GridTariffCell` (sourced from `allTimelines["grid"]`) is affected.
- The `AssetTimelinePoint` type update (`values` becoming nullable) is the only type change needed. No new response types or parsing logic required.
