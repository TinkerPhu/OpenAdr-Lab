# Implementation Plan: Reporter — Domain-Side Snapshot Types

**Branch**: `026-reporter-domain-types` | **Date**: 2026-05-15 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/026-reporter-domain-types/spec.md`

## Summary

`VEN/src/controller/reporter.rs` currently imports two infra-ring types — `HistoryPoint` (from `crate::assets`) and `SimState` (from `crate::simulator`) — violating Hexagonal Architecture's dependency rule. This plan replaces every `&SimState` parameter with domain-ring types (`AssetReportSample` map for history-heavy paths, `&SimSnapshot` for latest-state paths) and pushes the mapping work out to the three adapter-ring callers: `publish.rs`, `obligation.rs`, and `planning.rs`.

## Technical Context

**Language/Version**: Rust stable (2021 edition)
**Primary Dependencies**: `chrono`, `serde_json`, `std::collections::HashMap` (all existing — no new Cargo.toml entries)
**Storage**: N/A — no persistence changes
**Testing**: `wsl cargo check -p ven`, `wsl cargo test -p ven`, invariant grep commands
**Target Platform**: Linux ARM64 (Raspberry Pi 4), Docker Compose v2
**Project Type**: VEN backend service (Hexagonal + Clean Architecture)
**Performance Goals**: Simulator lock MUST be released before any reporter function runs
**Constraints**: No `VEN/src/` file > 500 lines; no logic changes to report-building algorithms; produced `OadrReportBody` payloads must remain bit-identical to pre-change output

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Notes |
|-----------|--------|-------|
| I — OpenADR Spec Fidelity | ✅ PASS | FR-008 prohibits wire-format changes. `OadrReportBody` fields, types, serialization unchanged. |
| II — BDD-First Testing | ✅ PASS | Existing BDD tests cover HTTP-level payload contract (SC-007). New `#[cfg(test)]` cargo tests cover domain function isolation (SC-004/005) — appropriate for an internal refactor with no new user-facing behavior. A BDD suite run is included as task T025 to verify SC-007 explicitly: all existing reporter-related scenarios must pass unchanged after the refactor. Constitution §VI requirement ("every refactoring phase MUST ship with tests that exercise the newly exposed test surface") is satisfied by SC-004/SC-005 unit tests for the new function signatures. |
| III — Upstream Compatibility | ✅ N/A | Changes are in VEN only — not in the `openleadr-rs` submodule. |
| IV — Lean Architecture | ✅ PASS | `AssetReportSample` is a 3-field struct. No new abstractions. `soc_from_point` removed when made redundant. |
| V — Infrastructure Parity | ✅ PASS | All tests run on Pi4-Server via Docker. Local: `wsl cargo check/test`. |
| VI — VEN Backend Hexagonal Architecture | ✅ PASS | This change IS the architecture fix. After applying, invariant greps return empty. |

No violations. Complexity Tracking table is not required.

## Project Structure

### Documentation (this feature)

```text
specs/026-reporter-domain-types/
├── plan.md              # This file
├── research.md          # Phase 0: code archaeology findings
├── data-model.md        # Phase 1: AssetReportSample entity design
└── tasks.md             # Phase 2 output (/speckit.tasks)
```

### Source Code (files changed by this feature)

```text
VEN/src/
├── controller/
│   └── reporter.rs            ← primary change (remove infra imports, new AssetReportSample,
│                                  update all function signatures, update all tests)
├── tasks/sim_tick/
│   └── publish.rs             ← callsite: run_measurement_reports extracts history, releases lock
├── services/
│   └── obligation.rs          ← callsite: check_and_report extracts history, releases lock
└── tasks/
    └── planning.rs            ← callsite: build_status_report now accepts &SimSnapshot
```

**Structure Decision**: Single VEN crate. All changes are within `VEN/src/`. No new files are needed — `AssetReportSample` is defined at the top of `reporter.rs`. No Cargo.toml changes.

## Phase 0: Research

See [research.md](research.md) for full findings. Key discoveries:

1. **All `&SimState` parameters in `reporter.rs`** (8 functions total — 3 public, 5 private)
2. **Three external callers** of public reporter functions — not just `publish.rs`:
   - `tasks/sim_tick/publish.rs` → `build_measurement_reports_for_active_events`
   - `services/obligation.rs` → `build_measurement_report_for_obligation`
   - `tasks/planning.rs` → `build_status_report`
3. **`build_measurement_report_for_obligation` uses full history** (2h window) — the `HashMap<String, Vec<AssetReportSample>>` approach handles this correctly since callers extract the full window
4. **`SimSnapshot.assets[id].val("soc")`** provides SoC for `build_status_report` (the `values` map on `AssetSnapshot`)
5. **Existing `#[cfg(test)]` block is large (560 lines)** — all tests must be rewritten to use domain types; `make_sim`/`make_entry`/`make_ev_entry` helpers are removed

## Phase 1: Design

See [data-model.md](data-model.md) for full entity design.

### New Type: `AssetReportSample`

```rust
/// Domain-ring sample: one history point for one asset, pre-extracted at the infra boundary.
/// Defined in `controller/reporter.rs`. Contains no infra-ring types.
pub struct AssetReportSample {
    pub ts: DateTime<Utc>,
    pub power_kw: f64,
    pub soc: Option<f64>,  // None for assets without SoC (PV, BaseLoad, Heater, Grid)
}
```

### Function Signature Changes (reporter.rs)

| Function | Before | After |
|----------|--------|-------|
| `build_measurement_report` | `(event, sim: &SimState, ven_name)` | `(event, asset_samples: &HashMap<String, Vec<AssetReportSample>>, grid_net_import_kw: f64, grid_net_export_kw: f64, ven_name)` |
| `build_measurement_reports_for_active_events` | `(events, sim: &SimState, ven_name, now)` | `(events, asset_samples: &HashMap<String, Vec<AssetReportSample>>, grid_net_import_kw: f64, grid_net_export_kw: f64, ven_name, now)` |
| `build_measurement_report_for_obligation` | `(obligation, sim: &SimState, ven_name, site_envelope)` | `(obligation, asset_samples: &HashMap<String, Vec<AssetReportSample>>, ven_name, site_envelope)` |
| `build_status_report` | `(event, sim: &SimState, ven_name, program_id, now)` | `(event, snap: &SimSnapshot, ven_name, program_id, now)` |
| `latest_net_import_kw` (private) | `(sim: &SimState) → f64` | `(snap: &SimSnapshot) → f64` (or inline) |
| `latest_net_export_kw` (private) | `(sim: &SimState) → f64` | removed — grid_net_export_kw passed directly to measurement report |
| `build_net_site_power_ts` (private) | `(sim: &SimState) → TimeSeries` | `(samples: &HashMap<String, Vec<AssetReportSample>>) → TimeSeries` |
| `build_soc_intervals` (private) | `(sim: &SimState, ...)` | `(samples: &HashMap<String, Vec<AssetReportSample>>, ...)` |
| `points_to_power_ts` (private) | `(points: &[HistoryPoint], ...)` | `(samples: &[AssetReportSample], ...)` |
| `soc_from_point` (private) | `(p: &HistoryPoint) → Option<f64>` | **removed** — `AssetReportSample.soc` is pre-computed |

### Callsite Changes

**`tasks/sim_tick/publish.rs` — `run_measurement_reports`**:
```
1. Add sim_snap: SimSnapshot parameter (passed from tick.rs which has tick_sim_snap)
2. Lock sim: extract HashMap<String, Vec<AssetReportSample>> for all assets (2h window)
3. Release lock
4. Compute grid_net_import_kw = sim_snap.assets.values().filter(kw > 0).sum()
5. Compute grid_net_export_kw = sim_snap.assets.values().filter(kw < 0).map(|kw| -kw).sum()
6. Call build_measurement_reports_for_active_events with domain types
```

**`services/obligation.rs` — `check_and_report`**:
```
1. Lock sim: extract HashMap<String, Vec<AssetReportSample>> for all assets (2h window)
2. Release lock (move lock release out of the existing block)
3. Call build_measurement_report_for_obligation with asset_samples (no sim ref)
```

**`tasks/planning.rs` — around line 234**:
```
1. Remove: let sim_snap = sim.lock().await.clone() (the re-lock)
2. Use the outer sim_snap: SimSnapshot (already computed earlier in the function)
3. Call build_status_report(&event, &sim_snap, ...) with SimSnapshot
```

### History Window

Both `publish.rs` and `obligation.rs` must use `Duration::hours(2)` when slicing history (same window as the current `build_net_site_power_ts` implementation). The `now` parameter available in both callers is used for the slice end point.

### No Contracts Needed

This is a pure internal refactoring. No HTTP endpoints, VTN API fields, or external data formats change.
