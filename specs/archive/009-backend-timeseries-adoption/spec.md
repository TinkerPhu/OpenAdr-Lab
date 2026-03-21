# Feature Specification: Backend Adoption of TimeSeries Resampling

**Feature Branch**: `009-backend-timeseries-adoption`
**Created**: 2026-03-21
**Status**: Draft
**Input**: User description: "RF-05b — Replace all ad-hoc time-series lookup functions in backend Rust code with TimeSeries resampling operations"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Planner tariff lookups use pre-resampled series (Priority: P1)

The HEMS planner currently looks up tariff values (import, export, CO2) one slot at a time by scanning the full `TariffSnapshot` list on every iteration. After this change, each tariff quantity is converted to a `TimeSeries` (Step interpolation) and resampled to the planner's slot grid **once** before the loop starts. Each slot then reads its tariff from the resampled array by index — no per-slot search, and tariffs spanning slot boundaries are correctly time-weighted.

**Why this priority**: This is the core value of RF-05b — eliminates per-slot O(n) scans, fixes boundary-crossing tariff accuracy, and sets the pattern for all other conversions.

**Independent Test**: Can be fully tested by running the planner with tariffs that span a slot boundary and verifying the slot receives the correct time-weighted average. Existing BDD scenarios that check plan output continue to pass with identical results for tariffs that don't cross boundaries.

**Acceptance Scenarios**:

1. **Given** a tariff series with a price change mid-slot (e.g., import changes from 0.20 to 0.15 at a point inside a 5-minute slot), **When** the planner builds the grid, **Then** the slot's `import_tariff_eur_kwh` equals the time-weighted average of the two prices across the slot duration.
2. **Given** a tariff series where all price changes align exactly on slot boundaries, **When** the planner builds the grid, **Then** slot tariff values are identical to the current ad-hoc lookup results (no regression).
3. **Given** a tariff series that does not cover all slots (gaps), **When** the planner builds the grid, **Then** slots without tariff coverage fall back to the configured default price, exactly as the current implementation does.

---

### User Story 2 - Asset forecast lookups use resampled series (Priority: P1)

The planner currently uses `nearest_value()` to look up PV forecasts per slot, with inconsistent logic for Step vs. Linear interpolation. After this change, asset forecast series are resampled to the slot grid using `resample_uniform(slot_width)` **once** before the loop, and the slot reads from the resampled output by index.

**Why this priority**: Eliminates the inconsistent `nearest_value()` function and unifies all per-slot lookups behind the same resampling abstraction.

**Independent Test**: Run the planner with a PV forecast and verify each slot's `pv_forecast_kw` matches the time-weighted average (for Linear) or LOCF value (for Step) over the slot interval.

**Acceptance Scenarios**:

1. **Given** a PV forecast with Linear interpolation and values at non-slot-aligned timestamps, **When** the planner builds the grid, **Then** each slot's PV value equals the time-weighted mean of the forecast across that slot's interval.
2. **Given** a baseline forecast with Step interpolation, **When** the planner builds the grid, **Then** each slot's baseline value equals the LOCF time-weighted value across the slot.
3. **Given** an asset with no forecast data (empty series), **When** the planner builds the grid, **Then** the slot value defaults to 0.0 (current behaviour preserved).

---

### User Story 3 - Tariff conversion at OpenADR interface boundary (Priority: P2)

When OpenADR event data arrives, the `TariffSnapshot` list is converted into three separate `TimeSeries` objects (import, export, CO2) with Step interpolation at the interface boundary. Downstream consumers (planner, reporter) receive `TimeSeries` instead of raw snapshot arrays.

**Why this priority**: Structural prerequisite for Stories 1 and 4 — the conversion must happen at the boundary so all downstream code works with the unified type.

**Independent Test**: Parse a known OpenADR event payload into `TimeSeries` and verify the series contains the expected samples with correct timestamps and values.

**Acceptance Scenarios**:

1. **Given** a parsed list of `TariffSnapshot`s with import, export, and CO2 values, **When** converted to `TimeSeries`, **Then** each series contains one sample per interval start, the interpolation mode is Step, and `interpolate_at()` returns the correct value at any timestamp within coverage.
2. **Given** a `TariffSnapshot` where some quantities are `None` (e.g., no CO2), **When** converted, **Then** the corresponding series omits those timestamps (only non-None values become samples).
3. **Given** overlapping or unsorted snapshots, **When** converted, **Then** the resulting series is sorted by timestamp with duplicates resolved by last-write-wins (matching current merge behaviour).

---

### User Story 4 - Report generation uses resampled history (Priority: P3)

Report obligations specify an interval (e.g., every 15 minutes). The reporter currently emits only the latest snapshot. After this change, the reporter resamples asset history to the obligation interval using `resample_uniform(obligation_interval)` and produces a report row per resampled point.

**Why this priority**: Improves report accuracy but has lower immediate impact since current reports work (single-snapshot). Full interval-aligned reporting is a correctness improvement for OpenADR compliance.

**Independent Test**: Feed asset history spanning multiple obligation intervals and verify the report contains one row per interval with correctly aggregated values.

**Acceptance Scenarios**:

1. **Given** an asset history buffer with 30 minutes of data and a 15-minute obligation interval, **When** the reporter generates a measurement report, **Then** the output contains 2 data points, each representing the time-weighted average power over its 15-minute window.
2. **Given** sparse history (gaps in data), **When** the reporter resamples, **Then** intervals with insufficient data are omitted rather than filled with incorrect values.

---

### Edge Cases

- What happens when the tariff series has exactly one sample? The Step interpolation carries it forward indefinitely — all slots receive that single tariff value.
- What happens when `resample_uniform(slot_width)` produces fewer points than expected slots? The planner must handle the shorter array gracefully (use default for any slot beyond the resampled range).
- What happens when the slot width does not evenly divide the tariff interval? The time-weighted mean correctly handles partial overlap — this is the primary correctness improvement of RF-05b.
- What happens when all tariff quantities are `None` for a snapshot? That snapshot contributes no samples to any of the three series.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST convert `TariffSnapshot` lists into three `TimeSeries` (import, export, CO2) with Step interpolation at the OpenADR interface boundary.
- **FR-002**: System MUST resample all tariff `TimeSeries` to the planner's slot grid using `resample_uniform(slot_width)` once before the slot loop begins.
- **FR-003**: System MUST resample all asset forecast `TimeSeries` to the planner's slot grid using `resample_uniform(slot_width)` once before the slot loop begins.
- **FR-004**: System MUST replace per-slot `tariff_import_at()`, `tariff_export_at()`, `tariff_co2_at()` calls with indexed reads from the pre-resampled arrays.
- **FR-005**: System MUST replace per-slot `nearest_value()` calls with indexed reads from the pre-resampled asset forecast arrays.
- **FR-006**: System MUST use default values (existing defaults) for any slot index that falls outside the resampled array range.
- **FR-007**: System MUST resample asset history to the obligation interval using `resample_uniform(obligation_interval)` when generating measurement reports.
- **FR-008**: System MUST produce identical planner output for tariff series that align exactly on slot boundaries (no regression).
- **FR-009**: System MUST produce correctly time-weighted tariff values for tariffs that span slot boundaries.
- **FR-010**: The planner's interface MUST accept tariff data as `TimeSeries` (not raw `TariffSnapshot` slices) after this change.

### Key Entities

- **TimeSeries**: Ordered sequence of (timestamp, value) samples with an interpolation mode (Step or Linear). Supports resampling to uniform grids via `resample_uniform()`. Already exists (RF-05a deliverable).
- **TariffSnapshot**: Current per-interval tariff record with `interval_start`, `interval_end`, and optional import/export/CO2 values. After RF-05b, this entity is only used at the parsing boundary and immediately converted to `TimeSeries`.
- **PlanTimeSlot**: Per-slot planner output. Fields unchanged — `import_tariff_eur_kwh`, `export_tariff_eur_kwh`, `co2_g_kwh` are populated from resampled series instead of ad-hoc lookups.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: All existing BDD scenarios (143+ scenarios, 895+ steps) pass without modification — zero regressions.
- **SC-002**: Planner output for tariffs aligned on slot boundaries is bit-for-bit identical to the previous implementation.
- **SC-003**: For a tariff that changes mid-slot, the planner produces the mathematically correct time-weighted average (verified by new unit tests).
- **SC-004**: The three ad-hoc tariff lookup functions (`tariff_import_at`, `tariff_export_at`, `tariff_co2_at`) and `nearest_value()` are fully removed from the codebase.
- **SC-005**: New unit tests cover: boundary-aligned tariffs, mid-slot tariff changes, empty tariff series, single-sample series, and gap handling — achieving full branch coverage of the conversion and lookup paths.
- **SC-006**: Report generation produces interval-aligned data points matching the obligation interval, verified by unit tests.

## Assumptions

- The `TimeSeries` resampling operations from RF-05a are complete, tested, and available (confirmed: `resample_uniform` and `resample_to_grid` exist in `common/mod.rs`).
- The planner's slot width is constant across all slots within a single plan cycle.
- Step interpolation with time-weighted mean is the correct aggregation for tariff values (price-like quantities). This matches the OpenADR convention that a price holds until the next price change.
- Linear interpolation with time-weighted mean is the correct aggregation for power forecasts (PV, baseline). This produces a smoother average that better reflects physical reality.
- The planner's interface change (accepting `TimeSeries` instead of `&[TariffSnapshot]`) is an internal refactor — no external consumers are affected.

## Dependencies

- **RF-05a** (TimeSeries resampling operations) — MUST be complete. Status: completed (commit ccac7c9).
