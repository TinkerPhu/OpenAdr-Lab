# Implementation Plan: Decouple PROFILE from Domain (Phase 4)

**Branch**: `021-decouple-profile-domain` | **Date**: 2026-05-11 | **Spec**: [spec.md](./spec.md)  
**Input**: Feature specification from `specs/021-decouple-profile-domain/spec.md`

## Summary

The VEN domain ring (`entities/`, `assets/`, `controller/`, `simulator/`) currently imports
`crate::profile` at 14 production sites to read YAML configuration structs directly. This couples
domain logic to a configuration format — breaking the Hexagonal Architecture dependency rule and
making domain unit tests impossible without a real YAML file.

Phase 4 extracts typed **domain parameter structs** that carry exactly the values each domain
component needs. The YAML profile stays in the infrastructure ring; the application layer
(`main.rs`) is the sole point where a `Profile` is read and parameter structs are assembled from it.
All 14 domain import sites are updated to use the new structs. No logic changes — purely structural.

The adjustment task ADJ-01 (move `PlannerObjective` to the domain ring) is the first
implementation step; it unblocks all subsequent profile-import removals in dispatcher, absorber,
plan, and the entire milp_planner submodule family.

## Technical Context

**Language/Version**: Rust stable 2021 edition  
**Primary Dependencies**: tokio (async runtime), axum (HTTP), serde/serde_yaml (infra ring only after Phase 4), good_lp / HiGHS (MILP solver — unchanged)  
**Storage**: N/A — no persistence schema changes; existing `/data/sim_state.json` format is unchanged  
**Testing**: `cargo test` (unit), `behave` BDD via Docker (integration/E2E)  
**Target Platform**: Linux ARM64 (Raspberry Pi 4), Docker Compose v2  
**Project Type**: Structural refactoring within existing Rust binary (`VEN/`)  
**Performance Goals**: No regressions — MILP solve time and sim tick latency must be unchanged  
**Constraints**: Zero behaviour change; no new crate dependencies; `routes/hems.rs` out of scope  
**Scale/Scope**: ~14 production import sites, ~5 new asset param structs, ~4 new cross-cutting param structs, 1 moved enum

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Gate | Status |
|-----------|------|--------|
| I — OpenADR Spec Fidelity | No OpenADR field names involved in this refactoring | ✅ N/A |
| II — BDD-First Testing | BDD suite must remain green; no new behaviour introduced — existing scenarios cover the runtime paths | ✅ Satisfied: existing BDD coverage applies; new unit tests added per FR-008 |
| III — Upstream Compatibility | VEN is not part of the `openleadr-rs` submodule; no upstream PR required | ✅ N/A |
| IV — Lean Architecture | New structs carry only what the domain needs — no extra abstraction layers, no service interfaces, no repository pattern | ✅ Satisfied |
| VI — VEN Hexagonal Architecture | This phase directly implements the constitution's invariant: domain code MUST NOT import `PROFILE` | ✅ This is the goal |
| VI — Line limit (500 lines) | `entities/planner_params.rs` will carry PlannerObjective + PlannerParams (28 fields) + AbsorberParams + SimulatorParams. Estimated ~150 lines. Each asset params struct: ~20 lines per asset file. All well under 500. | ✅ No violations |

**Constitution Check: PASSED — no violations, no Complexity Tracking entries required.**

## Project Structure

### Documentation (this feature)

```text
specs/021-decouple-profile-domain/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output — all new entities + field mappings
├── quickstart.md        # Phase 1 output — verification commands
└── tasks.md             # Phase 2 output (/speckit.tasks — not yet created)
```

### Source Code Layout

```text
VEN/src/
├── entities/
│   ├── mod.rs                  # add pub mod planner_params; re-export key types
│   ├── planner_params.rs       # NEW: PlannerObjective, PlannerParams, AbsorberParams,
│   │                           #      AbsorberAssetParams, SimulatorParams
│   └── [existing files — unchanged]
│
├── assets/
│   ├── battery.rs              # add BatteryParams; update from_config() / constructor
│   ├── ev.rs                   # add EvParams; update constructor
│   ├── heater.rs               # add HeaterParams (pre-resolved effective fields);
│   │                           #   update constructor; move forecast helper to PvParams
│   ├── pv.rs                   # add PvParams; move forecast_kw() here from PvConfig
│   └── base_load.rs            # add BaseLoadParams; update constructor
│
├── controller/
│   ├── absorber.rs             # use AbsorberParams (from entities/); remove profile import
│   ├── dispatcher.rs           # use PlannerObjective (from entities/); remove profile import
│   └── milp_planner/
│       ├── envelopes.rs        # use asset Params (from assets/); remove profile import
│       ├── inputs.rs           # use asset Params; remove profile import
│       ├── mod.rs              # use PlannerParams, PlannerObjective; remove profile import
│       ├── results.rs          # use PlannerObjective; remove profile import
│       └── types.rs            # use PlannerParams, PlannerObjective; remove profile import
│
├── simulator/
│   ├── mod.rs                  # replace from_profile(profile) → from_params(asset_params)
│   └── persist.rs              # use SimulatorParams; remove profile import
│
├── main.rs                     # sole Profile → domain params assembly site
│
└── profile.rs                  # infrastructure ring — unchanged except:
                                 #   PlannerObjective re-exported from entities/ (bridge
                                 #   removed when all callers updated; see research.md)
```

**Structure Decision**: Single Rust binary (VEN/src/). No new crate boundaries. New param structs
are co-located with their consumers (asset params in assets/ files) or placed in entities/ for
cross-cutting types. The application-layer assembly is a private helper function in `main.rs`.

## Complexity Tracking

No constitution violations require justification.
