# Implementation Plan: Uniform-Grid Timeline API

**Branch**: `010-uniform-grid-timeline` | **Date**: 2026-03-21 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/010-uniform-grid-timeline/spec.md`

## Summary

Replace the per-asset stride-based `downsample()` in `GET /timeline/all` and `GET /timeline/:asset_id` with a shared uniform time grid resampling. The response format stays `Record<string, {ts, values}[]>`. Each asset's array is three segments concatenated in ascending order: (1) history grid points with LOCF aggregation, (2) a single now-point with instantaneous values at exact `now`, (3) future grid points with step-interpolated plan data. Grid timestamps are snapped to round boundaries so the same resolution always produces the same grid. The `resolution` parameter (seconds) replaces `max_points` (kept as deprecated alias).

## Technical Context

**Language/Version**: Rust (stable, 2021 edition)
**Primary Dependencies**: axum (HTTP), chrono (timestamps), serde/serde_json, tokio
**Storage**: In-memory `AssetHistoryBuffer` (VecDeque, 3600 rows per asset)
**Testing**: cargo test (unit), Python behave BDD (integration), vitest (UI unit)
**Target Platform**: Linux ARM64 (Raspberry Pi 4), Docker Compose v2
**Project Type**: Web service (VEN backend)
**Performance Goals**: Response under 50ms for default 2-hour window with 5 assets
**Constraints**: ARM64 resource limits (1.5 CPU, 1500M memory); response < 100 KB
**Scale/Scope**: 3-5 assets, ~3600 history rows per asset, ~300 grid points default

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Notes |
|-----------|--------|-------|
| I. OpenADR Spec Fidelity | PASS | No OpenADR field names involved — timeline is an internal VEN API |
| II. BDD-First Testing | PASS | BDD scenarios will verify grid alignment and now-point |
| III. Upstream Compatibility | N/A | No openleadr-rs changes required |
| IV. Lean Architecture | PASS | Resampling is a single pure function; response shape unchanged; no new abstractions |
| V. Infrastructure Parity | PASS | Same Docker Compose; no infra changes |

## Project Structure

### Documentation (this feature)

```text
specs/010-uniform-grid-timeline/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/           # Phase 1 output
│   └── timeline-api.md
└── tasks.md             # Phase 2 output (/speckit.tasks)
```

### Source Code (repository root)

```text
VEN/src/
├── controller/
│   └── timeline.rs      # MODIFY: add resample_to_grid() for LOCF aggregation;
│                        #         add build_now_point() for instantaneous values;
│                        #         grid computation with round-boundary snapping
├── main.rs              # MODIFY: update get_timeline_all, get_timeline handlers;
│                        #         compute shared grid once, resample each asset;
│                        #         add resolution param to TimelineParams;
│                        #         replace downsample() with grid resampling + now-point
tests/
├── features/
│   └── timeline_grid.feature  # NEW: BDD scenarios for grid alignment + now-point
└── steps/
    └── timeline_steps.py      # NEW or EXTEND: step definitions for timeline tests
```

**Structure Decision**: All changes are in existing VEN backend files. New functions in `timeline.rs` for grid resampling and now-point extraction. HTTP handler changes in `main.rs`. No new crates or modules. Response format unchanged.

## Complexity Tracking

> No violations — no entries needed.
