# Feature Specification: Uniform-Grid Timeline API

**Feature Branch**: `010-uniform-grid-timeline`
**Created**: 2026-03-21
**Status**: Draft
**Input**: User description: "RF-05c from docs/BACKLOG.md — Backend: uniform-grid timeline API"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Grid-Aligned Multi-Asset Timeline (Priority: P1)

As a VEN UI user viewing the stacked area chart, I want all asset timelines returned on a shared uniform time grid so that chart series align by array position without client-side interpolation or nearest-match lookups.

**Why this priority**: This is the core problem — per-asset downsampling with different strides causes misaligned timestamps across assets, leading to false zero-spikes and chart rendering artifacts. Fixing this unlocks the entire RF-05d UI simplification.

**Independent Test**: Call `GET /timeline/all` and verify that every asset's array has the same length and identical `ts` values at each index position.

**Acceptance Scenarios**:

1. **Given** a VEN with 3 assets (ev, battery, base_load) running for 30 minutes, **When** calling `GET /timeline/all?hours_back=1&hours_forward=1`, **Then** the response is a `Record<string, {ts, values}[]>` where every asset's array has the same length and the same `ts` value at each index position.
2. **Given** the same VEN, **When** calling `GET /timeline/all` with default parameters, **Then** the grid portion of each asset's array has uniformly spaced `ts` values snapped to round boundaries (e.g., resolution=10s gives `11:00:00`, `11:00:10`, `11:00:20`, ...).
3. **Given** a VEN with plan data extending 1 hour into the future, **When** calling `GET /timeline/all?hours_forward=1`, **Then** future plan slots are step-interpolated onto the uniform grid — each grid bucket gets the plan slot value covering its start timestamp.

---

### User Story 2 - Now-Point with Instantaneous Values (Priority: P1)

As a VEN UI user, I want each asset's timeline to include a single now-point at the exact current timestamp with the asset's instantaneous values, so the chart and cursor display the real current state without the UI needing to interpolate.

**Why this priority**: The uniform grid snaps to round boundaries, so `now` almost never falls on a grid point. Without a now-point, the UI would need to interpolate between the two nearest grid points — but it doesn't know the interpolation method. The server owns the data and should provide the exact value.

**Independent Test**: Call `GET /timeline/all` and verify each asset's array contains one entry whose `ts` is the exact `now` timestamp, positioned between the last history grid point and the first future grid point.

**Acceptance Scenarios**:

1. **Given** a running VEN, **When** calling `GET /timeline/all`, **Then** each asset's array contains a now-point: a single entry between the history and future grid portions whose `ts` is the exact server `now` and whose `values` are the asset's instantaneous readings.
2. **Given** a VEN where `now` does NOT fall on a grid boundary, **When** inspecting the array, **Then** the now-point sits between the last history grid point and the first future grid point, preserving ascending sort order.
3. **Given** two consecutive calls to `GET /timeline/all` with the same `resolution`, **Then** the grid timestamps in the history and future portions are identical (deterministic grid), while the now-point's `ts` reflects each call's actual `now`.

---

### User Story 3 - Resolution Parameter (Priority: P2)

As an API consumer, I want to specify a `resolution` (in seconds) for the uniform grid bucket width, so I can control the density of the returned data.

**Why this priority**: Different views may need different granularity. The stacked chart works well at ~300 points, but a zoomed-in detail view may want finer resolution.

**Independent Test**: Call `GET /timeline/all?resolution=30` and verify the grid spacing is 30 seconds; call with `resolution=5` and verify 5-second spacing.

**Acceptance Scenarios**:

1. **Given** a running VEN, **When** calling `GET /timeline/all?resolution=30&hours_back=1&hours_forward=1`, **Then** consecutive grid-portion `ts` entries are exactly 30 seconds apart.
2. **Given** a request with no `resolution` parameter, **When** calling `GET /timeline/all?hours_back=1&hours_forward=1`, **Then** the system auto-calculates a resolution targeting approximately 300 data points for the total time window.
3. **Given** a request using the deprecated `max_points` parameter, **When** calling `GET /timeline/all?max_points=150`, **Then** the system converts `max_points` to an equivalent resolution and returns grid-aligned data.

---

### User Story 4 - Single-Asset Timeline Endpoint (Priority: P3)

As an API consumer, I want `GET /timeline/:asset_id` to also return grid-aligned data with the now-point for consistency.

**Why this priority**: While the main UI uses `/timeline/all`, the single-asset endpoint should apply the same uniform grid resampling.

**Independent Test**: Call `GET /timeline/ev` and verify the response has uniformly spaced `ts` values with a now-point.

**Acceptance Scenarios**:

1. **Given** a running VEN with an "ev" asset, **When** calling `GET /timeline/ev?hours_back=1&hours_forward=1`, **Then** the response is `[{ts, values}, ...]` with grid-aligned history, a now-point, and grid-aligned future.
2. **Given** a request for an unknown asset, **When** calling `GET /timeline/xyz`, **Then** the response is 404 with an error message (unchanged behavior).

---

### Edge Cases

- What happens when the time window produces fewer raw data points than the grid would contain? Empty grid buckets contain `null` values (the entry exists to keep arrays aligned, but `values` is `null`).
- What happens when `resolution` is extremely small (e.g., 1 second) for a large window? The system caps the maximum number of grid points to prevent excessive memory usage and response size.
- What happens when there is no plan data? Future grid buckets contain `null` values; the grid still covers the full requested window.
- What happens when `hours_back=0` and `hours_forward=0`? The response contains only the now-point for each asset.
- What happens when `max_points` and `resolution` are both specified? `resolution` takes precedence; `max_points` is ignored.
- What happens when `now` exactly falls on a grid boundary? The now-point coincides with the last history grid point — it can be deduplicated or kept as-is (implementation detail).

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: `GET /timeline/all` MUST return the existing response format `Record<string, {ts, values}[]>` — no structural change to the response shape.
- **FR-002**: All asset arrays in the response MUST have identical length and identical `ts` values at each index position.
- **FR-003**: Each asset's array MUST be structured as three concatenated segments in ascending time order: (1) history grid points, (2) a single now-point, (3) future grid points.
- **FR-004**: The grid timestamps MUST be snapped to round boundaries determined by the resolution (e.g., resolution=10s gives multiples of 10 seconds). The same resolution and time window MUST always produce the same grid, regardless of when the call is made.
- **FR-005**: The now-point MUST contain the asset's instantaneous current values at the exact server `now` timestamp. It is NOT grid-aligned.
- **FR-006**: The system MUST accept an optional `resolution` query parameter (in seconds) that sets the uniform grid bucket width. When omitted, the system MUST auto-calculate a resolution targeting approximately 300 grid points for the total time window.
- **FR-007**: The system MUST support the deprecated `max_points` query parameter as an alias — converting it to an equivalent `resolution` value. When both are specified, `resolution` takes precedence.
- **FR-008**: History buckets MUST aggregate raw data via time-weighted mean (last-observation-carried-forward within each bucket, then averaged).
- **FR-009**: Future (plan) buckets MUST use step interpolation — each bucket gets the plan slot value that covers its start timestamp.
- **FR-010**: Grid buckets with no data (no history rows and no plan slots covering them) MUST have `values: null` in the response entry.
- **FR-011**: The system MUST cap the maximum grid points per response to prevent excessive memory usage (cap at 3600 points).
- **FR-012**: `GET /timeline/:asset_id` MUST apply the same uniform grid resampling with now-point.
- **FR-013**: The `/tariffs` endpoint MUST NOT be resampled — tariff data is a sparse step function and renders correctly as-is.

### Key Entities

- **Uniform Time Grid**: A sequence of equally-spaced timestamps snapped to round boundaries, spanning the requested time window. Shared across all assets. Deterministic: same resolution + window = same grid.
- **Grid Bucket**: A time interval `[grid_ts, grid_ts + resolution)`. History rows in the interval are LOCF-aggregated; plan slots are step-interpolated. No data = `null`.
- **Now-Point**: A single entry per asset at the exact `now` timestamp with instantaneous values. Sits between history and future grid portions. Not grid-aligned.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: All asset arrays in a `/timeline/all` response have identical length and identical `ts` values at each index — verified by automated tests.
- **SC-002**: Grid-portion `ts` values are exactly `resolution` seconds apart and snapped to round boundaries — verified by automated tests.
- **SC-003**: The UI stacked area chart renders without false zero-spikes caused by timestamp misalignment — verified by visual inspection and existing BDD tests.
- **SC-004**: Response payload size for a default 2-hour window with 5 assets stays under 100 KB — verified by measurement.
- **SC-005**: The deprecated `max_points` parameter continues to produce valid responses — verified by backward-compatibility tests.
- **SC-006**: The now-point in each asset's array contains the asset's instantaneous values at the exact `now` timestamp — verified by automated tests.

## Assumptions

- RF-05a (TimeSeries resampling concepts) is not a hard dependency — the uniform grid logic here is self-contained and uses simple bucket aggregation rather than a generic TimeSeries abstraction.
- The ~300-point auto-resolution target is a reasonable default for the stacked area chart's typical display width (~1200px at 4px per data point).
- History timestamps are already aligned across assets within each tick (same `now` pushed for all assets in the sim loop), so cross-asset alignment in history data is guaranteed at the raw level.
- The grid cap of 3600 points (1 hour at 1-second resolution) is sufficient for all practical use cases.
- The UI already knows `now` via `Date.now()` client-side for cursor positioning — the now-point provides the *value* at now, not the *position*.
