# Implementation Plan: Backend Adoption of TimeSeries Resampling

**Branch**: `009-backend-timeseries-adoption` | **Date**: 2026-03-21 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/009-backend-timeseries-adoption/spec.md`

## Summary

Replace all ad-hoc per-slot tariff and forecast lookup functions in the VEN planner with pre-resampled `TimeSeries` arrays. Tariffs are converted from `TariffSnapshot` lists to three Step-interpolated `TimeSeries` at the OpenADR interface boundary. The planner resamples all series to its slot grid once before the loop, then reads values by HashMap lookup. This eliminates O(n) per-slot scans, fixes tariff accuracy when prices change mid-slot (time-weighted averaging), and unifies all time-series access behind the `TimeSeries` abstraction from RF-05a.

## Technical Context

**Language/Version**: Rust (stable, 2021 edition)
**Primary Dependencies**: chrono (timestamps), serde/serde_json, tokio (async runtime), axum (HTTP)
**Storage**: In-memory (no persistence changes)
**Testing**: `cargo test` (unit) + BDD behave suite on Pi4 Docker (143 scenarios, 895 steps)
**Target Platform**: Linux ARM64 (Pi4 Docker) + Windows dev
**Project Type**: Embedded web service (VEN controller)
**Performance Goals**: Planner completes within 1s for 24h horizon at 5-minute slots (288 slots)
**Constraints**: No new dependencies. No API surface changes (internal refactor only).
**Scale/Scope**: 5 Rust source files modified, ~4 functions removed, ~2 new functions/structs added

## Constitution Check

*No constitution file found. Proceeding with standard quality gates.*

**Quality gates applied**:
- No new external dependencies: PASS
- No persistence schema changes: PASS
- No public API changes: PASS (internal planner interface only)
- Backward-compatible output: PASS (Plan and PlanTimeSlot unchanged)
- All existing tests must pass: PASS (SC-001 requirement)

## Project Structure

### Documentation (this feature)

```text
specs/009-backend-timeseries-adoption/
├── spec.md
├── plan.md              # This file
├── research.md          # Phase 0: all design decisions
├── data-model.md        # Phase 1: entity changes
├── quickstart.md        # Phase 1: developer guide
├── contracts/
│   └── planner-interface.md  # Phase 1: interface contract
└── checklists/
    └── requirements.md  # Spec quality checklist
```

### Source Code (files modified)

```text
VEN/src/
├── common/
│   └── mod.rs                    # TimeSeries (unchanged — RF-05a)
├── controller/
│   ├── openadr_interface.rs      # + tariffs_to_timeseries() conversion
│   ├── planner.rs                # Signature change, pre-resample, remove 4 helpers
│   └── reporter.rs               # Resample history to obligation interval
├── entities/
│   └── tariff_snapshot.rs        # + TariffTimeSeries struct
└── main.rs                       # Convert tariffs before calling planner
```

**Structure Decision**: Pure refactor within existing VEN module structure. No new files, no new modules.

## Implementation Phases

### Phase A: TariffTimeSeries struct + conversion function

1. Add `TariffTimeSeries` struct to `entities/tariff_snapshot.rs`
2. Add `TariffTimeSeries::from_snapshots(&[TariffSnapshot]) -> Self` constructor
3. Add unit tests for conversion: normal case, None gaps, empty input, unsorted input, duplicates

### Phase B: Planner signature change + pre-resampling

1. Change `run_planner()` signature: `rates: &[TariffSnapshot]` → `tariffs: &TariffTimeSeries`
2. In `build_grid()`: resample all three tariff series to slot width, build HashMap<i64, f64> per quantity
3. Replace `tariff_import_at()` / `tariff_export_at()` / `tariff_co2_at()` calls with HashMap lookups
4. Update `rate_estimated` flag: check if all three series are empty
5. Remove the three `tariff_*_at()` helper functions
6. Add unit tests for: boundary-aligned tariffs, mid-slot tariff change (time-weighted), gaps, single-sample, empty

### Phase C: Asset forecast resampling

1. In `build_grid()`: resample each asset forecast to slot width before the loop
2. Replace `nearest_value()` calls with HashMap lookups from resampled forecasts
3. Remove `nearest_value()` helper function
4. Add unit tests for: PV Linear resampling, EV Step resampling, empty forecast, missing asset key

### Phase D: Update caller (main.rs)

1. In planning loop: convert `Vec<TariffSnapshot>` → `TariffTimeSeries` before calling `run_planner()`
2. Verify compilation and existing unit tests pass

### Phase E: Reporter resampling (P3)

1. In `build_measurement_report()`: resample asset history to obligation interval
2. Produce one report payload per resampled point instead of single latest snapshot
3. Add unit tests for multi-interval reports

### Phase F: Integration testing

1. Run full BDD suite — all 143+ scenarios must pass
2. Verify no output changes for boundary-aligned tariffs (regression check)

## Complexity Tracking

No constitution violations. No complexity justifications needed.
