# Implementation Plan: 016 — Refactor VEN Backend

**Branch**: `016-refactor-ven-backend` | **Date**: 2026-04-29 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `specs/016-refactor-ven-backend/spec.md`

## Summary

Remove 7 structural technical debts from `VEN/src/`: one dead dead file
(`controller/profile.rs`), one legacy dual-format YAML shim (`DeviceConfig`), three
sets of dead code (`AssetCapabilities` + `capabilities()`, a legacy `cancel_request`
branch, a `"boiler"`/`"heater"` magic-string dual-match), and an oversized 20-field
monolith (`InnerState` → 3 grouped sub-structs). All changes are behaviour-preserving.
No new dependencies. No API shape change. `state.json` JSON format unchanged (preserved via a private `PersistedVenState` assembly helper; see FR-013 and T041).

## Technical Context

**Language/Version**: Rust stable (2021 edition)  
**Primary Dependencies**: tokio, axum, serde/serde_json, serde_yaml, uuid, chrono,
good_lp/HiGHS — all existing; **no new dependencies**  
**Storage**: N/A — no new storage; JSON persistence via `state.json` is unchanged  
**Testing**: `cargo test --workspace` (unit + doc tests); Python `behave` BDD suite on Pi4
via `docker compose -f tests/docker-compose.test.yml run --build --rm test-runner`  
**Target Platform**: Linux ARM64 (Raspberry Pi 4), Docker Compose v2  
**Project Type**: Rust HTTP service — VEN backend in a multi-service Docker Compose stack  
**Performance Goals**: No regression; HTTP handlers must not block under MILP solve
(snapshot-and-release pattern maintained)  
**Constraints**: Zero behaviour change (FR-015); HTTP API surface unchanged (FR-010);
`state.json` JSON format backwards-compatible (FR-013 via `PersistedVenState` helper assembling three separate lock snapshots)
**Scale/Scope**: ~7 Rust source files in `VEN/src/`; net result ≈ 200–350 lines deleted,
0 lines of new logic

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Gate Result | Notes |
|-----------|-------------|-------|
| I — OpenADR Spec Fidelity | ✅ CLEAR | No OpenADR field names or protocol messages touched |
| II — BDD-First Testing | ✅ CLEAR | No new behaviour; existing BDD suite is the regression guard; constitution exempts refactors from new scenario requirement |
| III — Upstream Compatibility | ✅ CLEAR | `openleadr-rs` submodule untouched; no upstream PR needed |
| IV — Lean Architecture | ✅ CLEAR | Net reduction: 7 dead elements removed. Three separate `Arc<RwLock<T>>` replace one monolithic lock; `PersistedVenState` (private helper) keeps JSON backwards-compatible without growing the public API surface |
| V — Infrastructure Parity | ✅ CLEAR | No Dockerfile, compose, or CI changes; Pi4 test run unchanged |

No violations — Complexity Tracking table not required.

**Post-design re-check**: All 5 principles still clear after Phase 1 design — confirmed
in research.md and data-model.md.

## Project Structure

### Documentation (this feature)

```text
specs/016-refactor-ven-backend/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/
│   └── http-api.md      # Phase 1 output
└── tasks.md             # Phase 2 output (speckit.tasks — NOT created here)
```

### Source Code

```text
VEN/src/
├── ids.rs                         # NEW: asset-ID string constants (R-03, R-08)
├── main.rs                        # MODIFIED: add mod ids; (R-03)
├── profile.rs                     # MODIFIED: remove DeviceConfig + fallback arms in 5 accessors (R-02)
├── assets/mod.rs                  # MODIFIED: delete AssetCapabilities, EnergyState, TimeWindow,
│                                  #           capabilities() impls (R-04)
├── state.rs                       # MODIFIED: split InnerState → 3 sub-structs (R-06);
│                                  #           remove legacy None branch in cancel_request (R-07)
├── routes/hems.rs                 # MODIFIED: "boiler"/"heater" literals → ids::BOILER/HEATER (R-03, R-08)
└── controller/
    └── profile.rs                 # DELETED (R-01)
```

**Structure Decision**: Single-service Rust backend. All changes are in `VEN/src/`.
No new directories. One new file (`ids.rs`). One deleted file (`controller/profile.rs`).
