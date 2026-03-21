# Implementation Plan: Planner Slot Costing — Configurable Aggregation

**Branch**: `013-slot-costing-aggregation` | **Date**: 2026-03-21 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/013-slot-costing-aggregation/spec.md`

## Summary

Add configurable aggregation (Mean/Min/Max) to `TimeSeries::resample_uniform()` so that
different quantities use semantically correct bucket reduction: time-weighted mean for
tariffs/prices, minimum for capacity limits (strictest value in slot). This is pure Rust
backend work in the VEN crate — no UI, no API, no persistence changes.

## Technical Context

**Language/Version**: Rust (stable, 2021 edition)
**Primary Dependencies**: chrono (timestamps)
**Storage**: N/A (in-memory TimeSeries only)
**Testing**: cargo test (unit tests in `VEN/src/common/mod.rs`)
**Target Platform**: Linux ARM64 (Pi4) + Windows dev
**Project Type**: Library module within VEN binary crate
**Performance Goals**: N/A (resampling runs once per plan cycle, sub-millisecond)
**Constraints**: No new dependencies; no API surface changes
**Scale/Scope**: 2 files changed, ~150 lines added

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Notes |
|-----------|--------|-------|
| I. OpenADR Spec Fidelity | N/A | No OpenADR field names involved — internal resampling module |
| II. BDD-First Testing | PASS | This is internal infrastructure with no user-facing behavior change. Unit tests (cargo test) are the appropriate level. No BDD scenarios needed — existing BDD suite must pass unchanged. |
| III. Upstream Compatibility | N/A | No changes to openleadr-rs submodule |
| IV. Lean Architecture | PASS | Minimal addition: one enum + one generic helper method. No abstractions beyond what RF-06 requires. `bucket_extreme` deduplicates min/max logic — justified since they differ by a single closure. |
| V. Infrastructure Parity | PASS | No Docker/infrastructure changes |

## Project Structure

### Documentation (this feature)

```text
specs/013-slot-costing-aggregation/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
└── checklists/
    └── requirements.md  # Spec quality checklist
```

### Source Code (repository root)

```text
VEN/src/
├── common/
│   └── mod.rs           # Aggregation enum, bucket_min/max, updated resample_uniform
└── controller/
    └── planner.rs       # Updated call sites (Aggregation::Mean)
```

**Structure Decision**: No new files or directories. Changes are confined to two existing
Rust source files in the VEN crate.

## Complexity Tracking

No constitution violations. Table not needed.
