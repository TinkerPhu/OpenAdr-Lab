# Feature Specification: Reporter Multi-Interval Resampling (RF-05e)

**Feature Branch**: `012-reporter-resampling`
**Created**: 2026-03-21
**Status**: Draft
**Input**: User description: "RF-05e — Reporter adoption: multi-interval resampling for measurement reports"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Multi-Interval Measurement Reports (Priority: P1)

As a VTN operator receiving measurement reports from a VEN, I need each report to contain one data row per obligation interval (e.g. every 15 minutes) covering the full reporting period, so that I can see how a site's power consumption evolved over time rather than receiving only a single latest-snapshot value.

**Why this priority**: The current reporter emits a single data point regardless of how long the reporting period is. This makes measurement reports fundamentally incomplete — a VTN that expects 4 rows for a 1-hour period with 15-minute intervals receives only 1 row. This is the core value of RF-05e.

**Independent Test**: Can be fully tested by triggering a measurement report for an event with a known obligation interval, then inspecting the report payload to verify it contains multiple interval rows with correctly aggregated power values.

**Acceptance Scenarios**:

1. **Given** an active event with a reportDescriptor specifying a 15-minute interval and 1 hour of accumulated asset history, **When** the reporter builds a measurement report, **Then** the report contains 4 interval payloads (one per 15-minute bucket), each with the time-weighted mean of net import power during that bucket.
2. **Given** an active event with a reportDescriptor specifying a 1-hour interval and 2 hours of history, **When** the reporter builds a measurement report, **Then** the report contains 2 interval payloads.
3. **Given** an active event with no reportDescriptor (no obligation interval specified), **When** the reporter builds a measurement report, **Then** the report falls back to a single interval containing the latest snapshot (backward-compatible behaviour).

---

### User Story 2 - Per-Asset-Type Conversion to Scalar Time Series (Priority: P1)

As a VEN operator, I need the reporter to correctly convert each asset's multi-keyed history buffer (power, SoC, temperature, etc.) into the appropriate scalar time series for the report payload type, so that each quantity is aggregated with the correct semantics.

**Why this priority**: Without this conversion, the reporter cannot produce correctly typed interval data. Power values need time-weighted averaging while SoC needs point-in-time sampling — using the wrong method produces incorrect reports.

**Independent Test**: Can be tested by populating an asset history buffer with known values and verifying that the conversion produces a TimeSeries with the expected samples and interpolation mode.

**Acceptance Scenarios**:

1. **Given** a battery asset with history containing both `power_kw` and `soc` columns, **When** the reporter converts history to a scalar time series for a USAGE report, **Then** it extracts the `power_kw` column as a Step-interpolated TimeSeries.
2. **Given** an EV asset with history containing `soc` values, **When** the reporter converts history for a STORAGE_CHARGE_LEVEL report, **Then** it extracts the `soc` column as a Linear-interpolated TimeSeries (point-in-time sampling, no time-weighted mean).
3. **Given** an asset with sparse history (some rows missing the target column), **When** the reporter converts to a time series, **Then** NaN rows are skipped and the resulting TimeSeries contains only valid samples.

---

### User Story 3 - Import/Export Power Split per Interval (Priority: P2)

As a VTN operator, I need the reporter to correctly split net power into import and export components per interval, so that IMPORT_CAPACITY_LIMIT and EXPORT_CAPACITY_LIMIT event reports reflect directional usage within each bucket.

**Why this priority**: Currently the reporter applies the import/export split on the latest snapshot only. With multi-interval support, the split must happen per resampled bucket. This is important for correctness but builds on top of the core multi-interval capability (P1).

**Independent Test**: Can be tested by providing asset history where power alternates between import (positive) and export (negative) across intervals, then verifying the report shows the correct directional values per bucket.

**Acceptance Scenarios**:

1. **Given** a site that imported 3 kW in the first 15-minute interval and exported 2 kW in the second, **When** the reporter builds an IMPORT_CAPACITY_LIMIT report, **Then** interval 1 shows 3000 W and interval 2 shows 0 W (only import counted).
2. **Given** a site that exported throughout an interval, **When** the reporter builds an EXPORT_CAPACITY_LIMIT report, **Then** the interval shows the absolute export value in watts.

---

### User Story 4 - Obligation Interval Plumbing (Priority: P1)

As a system maintainer, I need the obligation interval duration from the event's reportDescriptor to be available to the reporter at report-building time, so that the reporter knows what bucket width to use for resampling.

**Why this priority**: Without this plumbing, the reporter has no way to know the required interval width. This is a prerequisite for multi-interval reports.

**Independent Test**: Can be tested by verifying that the `interval_duration_s` from `OadrReportObligation` is passed through to the report builder and used as the resampling bucket width.

**Acceptance Scenarios**:

1. **Given** an event with a reportDescriptor specifying `"duration": "PT15M"`, **When** the system parses report obligations, **Then** the resulting `OadrReportObligation` has `interval_duration_s = 900`.
2. **Given** an obligation with `interval_duration_s = 900`, **When** the reporter builds a measurement report using this obligation, **Then** the resampling bucket width is 15 minutes.

---

### Edge Cases

- What happens when the asset history buffer contains less data than one full obligation interval? The reporter produces zero interval rows for that asset (no partial buckets).
- What happens when the obligation interval is very small (e.g. 10 seconds) relative to the history tick rate (1 second)? The reporter produces many small buckets, each containing the time-weighted mean of the samples within.
- What happens when no assets have any history at all? The report is still generated but with zero-valued interval payloads (matching current behaviour for missing data).
- How does the system handle an event that becomes active mid-interval? The first bucket starts at the next grid-aligned boundary after history begins, so partial leading intervals are excluded.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The reporter MUST accept an obligation interval duration and use it as the resampling bucket width when building measurement reports.
- **FR-002**: The reporter MUST convert `AssetHistoryBuffer` multi-keyed rows into scalar `TimeSeries` instances, selecting the appropriate column based on the report payload type (e.g. `power_kw` for USAGE, `soc` for STORAGE_CHARGE_LEVEL).
- **FR-003**: Power time series MUST use Step interpolation and time-weighted mean aggregation (consistent with the existing `resample_uniform` semantics).
- **FR-004**: SoC (state-of-charge) time series MUST use Linear interpolation for point-in-time sampling rather than time-weighted averaging.
- **FR-005**: The report payload MUST contain one interval entry per resampled bucket, each with its own `id` (sequential from 0) and aggregated `payloads`.
- **FR-006**: For IMPORT_CAPACITY_LIMIT reports, each interval MUST report only the positive (import) component of net power; for EXPORT_CAPACITY_LIMIT reports, each interval MUST report only the absolute negative (export) component.
- **FR-007**: When no obligation interval is specified (no reportDescriptor or missing duration), the reporter MUST fall back to a single interval containing the latest snapshot value (backward compatibility).
- **FR-008**: NaN values in the asset history buffer MUST be excluded when constructing the scalar TimeSeries (treated as missing data, not zero).
- **FR-009**: The reporter MUST continue to include OPERATING_STATE and STORAGE_CHARGE_LEVEL payloads alongside USAGE payloads (preserving existing report structure).
- **FR-010**: The call site in the tick loop MUST pass obligation information (interval duration) to the reporter when available.

### Key Entities

- **OadrReportObligation**: Existing entity representing a pending report obligation. Already carries `interval_duration_s`. The reporter must consume this field.
- **TimeSeries**: Existing resampling container with `resample_uniform(width)`. The reporter converts asset history into these for aggregation.
- **AssetHistoryBuffer**: Existing per-asset ring buffer of timestamped multi-keyed rows. The reporter extracts scalar columns from this.
- **Measurement Report Interval**: A single time bucket in the output report, containing aggregated payloads for one obligation interval.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Given 1 hour of 1-second asset history and a 15-minute obligation interval, the reporter produces exactly 4 interval rows per report.
- **SC-002**: Time-weighted mean values in report intervals match the expected aggregation to within 0.1% relative error.
- **SC-003**: Reports for events without reportDescriptors produce a single-interval report identical to the current behaviour (zero regression).
- **SC-004**: EV SoC values in report intervals reflect point-in-time values (not time-weighted means) at interval boundaries.
- **SC-005**: All existing BDD scenarios that create or verify reports continue to pass without modification.

## Assumptions

- The existing `TimeSeries::resample_uniform()` in `common/mod.rs` (from RF-05a) is correct and available for use.
- The `OadrReportObligation` entity already stores `interval_duration_s` parsed from the event's reportDescriptor — no new parsing logic is needed.
- The reporter's timer-driven call frequency (controlled by `report_interval_s` in the profile) remains unchanged — this feature changes the content of each report, not how often reports are sent.
- The VTN accepts reports with multiple intervals in the `resources[].intervals[]` array (per OpenADR 3.0 spec section 5.3).
