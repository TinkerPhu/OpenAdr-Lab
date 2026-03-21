# Feature Specification: Planner Slot Costing — Time-weighted Tariff & Min-Aggregated Capacity Limits

**Feature Branch**: `013-slot-costing-aggregation`
**Created**: 2026-03-21
**Status**: Draft
**Input**: User description: "RF-06 from BACKLOG.md — Planner slot costing: time-weighted tariff across slot boundaries"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Accurate slot cost when tariff changes mid-slot (Priority: P1)

As a HEMS planner, when a tariff rate changes partway through a planning slot, the planner must compute a time-weighted average cost for that slot so that energy scheduling decisions reflect the true blended price.

**Why this priority**: Cost accuracy is the foundation of all scheduling decisions. If a 5-minute slot straddles a tariff boundary (e.g., cheap-to-expensive at the hour mark), the planner must use the blended rate — not just the start-of-slot rate — to avoid systematic over- or under-scheduling.

**Independent Test**: Can be verified with a unit test that constructs a tariff series with a boundary inside a slot and checks that the resampled value matches the hand-calculated time-weighted average.

**Acceptance Scenarios**:

1. **Given** a tariff of €0.20/kWh from 10:00 and €0.15/kWh from 11:00 (step series), **When** the planner resamples to 5-minute slots, **Then** the slot starting at 10:55 has value €0.20 (entirely within the €0.20 interval) and the slot starting at 11:00 has value €0.15.
2. **Given** a tariff of €0.20/kWh from 10:00 and €0.15/kWh from 10:03 (step series), **When** the planner resamples to a 5-minute slot starting at 10:00, **Then** the blended tariff is (3min × €0.20 + 2min × €0.15) / 5min = €0.18.
3. **Given** a tariff series with no boundary within a slot, **When** the planner resamples, **Then** the slot value equals the single tariff that covers the entire slot.

---

### User Story 2 - Strictest capacity limit applied per slot (Priority: P1)

As a HEMS planner, when a capacity limit (import or export) changes partway through a planning slot, the planner must use the minimum (strictest) limit that applies anywhere within that slot. This prevents the planner from scheduling power levels that would violate the limit during any sub-interval.

**Why this priority**: Using the average capacity limit would allow the planner to schedule power that exceeds the limit during part of the slot — a safety and compliance violation. The strictest value must apply.

**Independent Test**: Can be verified with a unit test that constructs a capacity-limit series with a drop mid-slot and checks that the resampled value equals the lower (stricter) limit.

**Acceptance Scenarios**:

1. **Given** an import capacity limit of 10 kW from 10:00 that drops to 5 kW at 10:57, **When** the planner resamples to 5-minute slots using min aggregation, **Then** the slot starting at 10:55 has value 5.0 kW (the strictest limit in the bucket).
2. **Given** a capacity limit that is constant throughout a slot, **When** the planner resamples using min aggregation, **Then** the slot value equals that constant limit.
3. **Given** a capacity limit that increases mid-slot (e.g., 5 kW → 10 kW), **When** the planner resamples using min aggregation, **Then** the slot value is 5.0 kW (the lower, stricter value).

---

### User Story 3 - Configurable aggregation mode for resampling (Priority: P2)

As a developer extending the planner or reporter, I can choose the aggregation mode (mean, min, or max) when resampling a time series to a uniform grid, so that different quantities use the semantically correct aggregation.

**Why this priority**: This is the enabler for stories 1 and 2. Different physical quantities require different aggregation semantics: prices need time-weighted mean, capacity limits need min, and future quantities (e.g., peak demand tracking) may need max.

**Independent Test**: Can be verified with unit tests that resample the same series with each aggregation mode and assert the results differ as expected.

**Acceptance Scenarios**:

1. **Given** a step series with a value change mid-bucket, **When** resampled with mean aggregation, **Then** the result is the time-weighted average of the two values.
2. **Given** the same series, **When** resampled with min aggregation, **Then** the result is the lower of the two values.
3. **Given** the same series, **When** resampled with max aggregation, **Then** the result is the higher of the two values.
4. **Given** a constant series, **When** resampled with any aggregation mode, **Then** the result is the same constant value for all modes.

---

### Edge Cases

- What happens when a slot has no tariff data (before the first sample)? The slot is skipped (no output point), same as current behavior.
- What happens when a capacity limit series has only one sample? Min aggregation returns that single value for all slots within LOCF range — correct behavior for step series.
- What happens with a zero-width bucket? Returns no output — guarded by the `width_ms <= 0` check.
- How does linear interpolation interact with min/max? Each sub-segment between samples is monotonic, so extremes occur at segment endpoints. Checking values at all split points is sufficient.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The resampling function MUST support three aggregation modes: time-weighted mean, minimum, and maximum.
- **FR-002**: Time-weighted mean aggregation MUST compute the integral of the signal over each bucket divided by the bucket width, correctly handling multiple value changes within a single bucket.
- **FR-003**: Min aggregation MUST return the lowest signal value that occurs at any point within the bucket interval.
- **FR-004**: Max aggregation MUST return the highest signal value that occurs at any point within the bucket interval.
- **FR-005**: For step-interpolated series, min/max MUST check the value at the bucket start and at every interior change-point.
- **FR-006**: For linearly-interpolated series, min/max MUST additionally check the value at the bucket end boundary (since linear segments produce different values at each endpoint).
- **FR-007**: Tariff resampling in the planner MUST use mean aggregation (preserving current behavior).
- **FR-008**: Capacity-limit resampling (when capacity limits become time-varying series) MUST use min aggregation.
- **FR-009**: All existing planner behavior MUST remain unchanged — the aggregation parameter addition is backward-compatible with mean as the default semantic.

### Key Entities

- **Aggregation Mode**: An enumeration (Mean, Min, Max) that controls how values within a resampling bucket are combined.
- **TimeSeries**: An ordered sequence of timestamped values with an interpolation mode (Step or Linear). Supports resampling to uniform grids with configurable aggregation.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: All existing planner unit tests pass without modification to their assertions (only the aggregation parameter is added to call sites).
- **SC-002**: A tariff series spanning a slot boundary produces the mathematically correct time-weighted average (verified to within 1e-9 tolerance).
- **SC-003**: A capacity-limit series with a mid-slot drop produces the minimum (strictest) value when resampled with min aggregation.
- **SC-004**: Min, max, and mean aggregation produce identical results for constant-value series.
- **SC-005**: The RF-06 backlog verification example — slot [10:55, 11:00) with tariff [10:00=€0.20, 11:00=€0.15] — produces exactly €0.20 with mean aggregation.

## Assumptions

- Capacity limits (`import_limit_kw`, `export_limit_kw`) are currently scalar values in the planner. RF-05b will convert them to TimeSeries. This spec covers the aggregation infrastructure; the planner integration of time-varying capacity limits is deferred to RF-05b.
- Step interpolation is the correct mode for both tariffs and capacity limits (values change discretely at specific times, not continuously).
