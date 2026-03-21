# Implementation Plan: Reporter Multi-Interval Resampling (RF-05e)

**Branch**: `012-reporter-resampling` | **Date**: 2026-03-21 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/012-reporter-resampling/spec.md`

## Summary

Refactor `build_measurement_report()` in `VEN/src/controller/reporter.rs` to produce multi-interval measurement reports by resampling asset history onto obligation interval boundaries using the existing `TimeSeries::resample_uniform()`. Currently the reporter emits a single snapshot per report; after this change it emits one row per obligation interval with correctly aggregated values. The obligation interval duration is already stored in `OadrReportObligation.interval_duration_s` and needs to be plumbed to the report builder.

## Technical Context

**Language/Version**: Rust (stable, 2021 edition)
**Primary Dependencies**: chrono (timestamps), serde_json (report payloads), uuid, tokio (async runtime), axum (HTTP)
**Storage**: In-memory ring buffers (`AssetHistoryBuffer` — VecDeque, 3600 rows)
**Testing**: cargo test (unit), Python behave (BDD integration)
**Target Platform**: Linux ARM64 (Raspberry Pi 4), Docker
**Project Type**: VEN backend service
**Performance Goals**: N/A — reporter runs once per `report_interval_s` (typically 60s); resampling 3600 rows is negligible
**Constraints**: Must not change the report JSON structure visible to the VTN except adding more interval entries
**Scale/Scope**: Single module change (`reporter.rs`) + call-site plumbing in `main.rs`

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Notes |
|-----------|--------|-------|
| I. OpenADR Spec Fidelity | PASS | Report field names (`payloadType`, `resourceName`, `intervals`, etc.) remain unchanged. No DTO normalization. |
| II. BDD-First Testing | PASS | BDD scenarios will be written for multi-interval report content before implementation. Existing report scenarios preserved. |
| III. Upstream Compatibility | N/A | This change is in VEN application code, not openleadr-rs submodule. |
| IV. Lean Architecture | PASS | Reuses existing `TimeSeries::resample_uniform()` from RF-05a. No new abstractions — just a conversion function from `AssetHistoryBuffer` rows to `TimeSeries` and a loop over resampled buckets. |
| V. Infrastructure Parity | PASS | No Docker or infrastructure changes. Same test environment. |

## Project Structure

### Documentation (this feature)

```text
specs/012-reporter-resampling/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
└── tasks.md             # Phase 2 output (/speckit.tasks)
```

### Source Code (repository root)

```text
VEN/src/
├── common/
│   └── mod.rs           # TimeSeries, resample_uniform() — existing, no changes
├── controller/
│   ├── reporter.rs      # PRIMARY CHANGE: multi-interval report builder
│   └── openadr_interface.rs  # OadrReportObligation extraction — existing, no changes
├── entities/
│   └── capacity.rs      # OadrReportObligation struct — existing, no changes
└── main.rs              # Call-site: pass obligation interval to reporter

tests/features/
└── reporter_resampling.feature  # New BDD scenarios
tests/steps/
└── reporter_steps.py    # New/updated step definitions
```

**Structure Decision**: Pure backend refactor within existing module layout. No new modules or directories needed.

## Complexity Tracking

No constitution violations — table not needed.
