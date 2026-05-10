# Implementation Plan: MILP Asset Port — Decouple Planner from Concrete Asset Types

**Branch**: `020-milp-asset-port` | **Date**: 2026-05-10 | **Spec**: [spec.md](spec.md)

## Summary

Introduce the `AssetMilpContext` port trait and `AssetKind` discriminant into the VEN MILP planner to eliminate all direct imports of `Battery`, `EvCharger`, and `Heater` from `controller/milp/` and `controller/milp_interactions.rs`. Both the constraint-builder (solver files) and the cross-asset interaction module will receive `Vec<Box<dyn AssetMilpContext>>` instead of concrete types. As part of this, the LP variable handle types (`*MilpVars`) are relocated from `assets/` to `controller/milp_interactions.rs`, and three new `#[cfg(test)]` blocks (one per concrete asset implementation) are added, plus a new n=48 medium regression test profile.

## Technical Context

**Language/Version**: Rust stable (2021 edition)  
**Primary Dependencies**: `good_lp` (HiGHS MILP solver), `tokio`, `axum`, `serde`, `chrono`  
**Storage**: N/A — no persistence changes  
**Testing**: `cargo test` — `#[cfg(test)]` blocks in `controller/milp_planner/` and `assets/`; existing BDD suite as regression safety net  
**Target Platform**: Linux ARM64 (Raspberry Pi 4), Docker Compose v2  
**Project Type**: Embedded library (VEN backend service)  
**Performance Goals**: MILP solve time unchanged (10–24 s on Pi4 for n=288); new n=48 regression test must complete in < 5 s  
**Constraints**: 500-line file limit per Constitution VI; no `use crate::assets::` in any file under `VEN/src/controller/milp_planner/`; no `use crate::profile::` in any file under `VEN/src/controller/` or `VEN/src/entities/`  
**Scale/Scope**: 3 concrete asset implementations (Battery, EV, Heater); 1 cross-asset interaction (BatEvCoexist)

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Notes |
|-----------|--------|-------|
| I — OpenADR Spec Fidelity | ✅ N/A | Internal Rust refactoring; no OpenADR field names involved |
| II — BDD-First Testing | ✅ Pass | No new behaviour introduced; existing 232 BDD scenarios are the regression gate. New unit tests added for newly exposed test surface (Constitution VI mandate) |
| III — Upstream Compatibility | ✅ N/A | VEN backend only; not a submodule change |
| IV — Lean Architecture | ✅ Justified | `AssetMilpContext` trait is explicitly mandated by Constitution VI and the refactoring plan. Not speculative — the concrete need (adding an asset without touching two planner files) is stated in FR-001 |
| V — Infrastructure Parity | ✅ Pass | No Docker Compose or config changes |
| VI — VEN Hexagonal Architecture | ✅ This phase directly satisfies the mandate: `milp_planner` and `milp_interactions` MUST accept `Vec<Box<dyn AssetMilpContext>>`; direct imports of `A_BAT`, `A_EV`, `A_HTR` prohibited. Post-phase verifiable invariant must pass: `grep -r "use crate::assets::" VEN/src/controller/milp_planner → empty` |

**Post-Phase 1 re-check**: Pass — contracts and data model confirm no constitution violations.

## Complexity Tracking

*Required by Constitution IV: every non-trivial abstraction must be justified here before introduction.*

| Abstraction | Justification | Constitution Reference |
|-------------|---------------|----------------------|
| `AssetMilpContext` trait (port) | Directly mandated to remediate AB-02: `milp_planner` and `milp_interactions` MUST accept `Vec<Box<dyn AssetMilpContext>>`; direct imports of `A_BAT`, `A_EV`, `A_HTR` prohibited. Trait and all MILP asset types defined in `milp_planner/asset_port.rs`. | Constitution VI |
| `AssetKind` enum | Required by the port contract for pool-slot dispatch and structured logging without concrete type imports | Constitution VI |
| `AssetMilpParams` enum | Carries scalar parameters to `build_milp_inputs()`; replaces direct asset-type construction in `inputs.rs` without introducing a DTO layer | Constitution IV (no DTO normalisation) |
| Mock adapters in `services/test_support/` | Mandated test infrastructure: mock adapters MUST live in `VEN/src/services/test_support/`, compiled in all builds (not `#[cfg(test)]`), to be shareable across service test modules | Constitution VI |

## Project Structure

### Documentation (this feature)

```text
specs/020-milp-asset-port/
├── plan.md              ← this file
├── research.md          ← Phase 0 decisions
├── data-model.md        ← trait signatures, type inventory, call-flow diagram
├── contracts/
│   └── asset_milp_context.md   ← AssetMilpContext port contract
└── tasks.md             ← Phase 2 output (created by /speckit.tasks)
```

### Source Code changes

```text
VEN/src/controller/
├── milp_planner/                 ← existing sub-module directory
│   ├── mod.rs                    CHANGE: remove all use crate::assets::* imports (currently #[allow(unused_imports)]); add re-export of AssetMilpContext, AssetKind
│   ├── asset_port.rs             NEW: AssetKind enum, AssetMilpParams enum, AssetMilpContext trait;
│   │                                  BatteryMilpContext/Vars/SolOutput, EvMilpContext/Vars/SolOutput,
│   │                                  HeaterMilpContext/Vars/SolOutput struct definitions (moved from assets/)
│   ├── inputs.rs                 CHANGE: replace per-asset concrete-type construction with trait dispatch via AssetMilpParams
│   ├── solver_phase1.rs          CHANGE: remove BatteryMilpContext/EvMilpContext/HeaterMilpContext reconstructions; call asset.declare_vars_into_pool() instead
│   ├── solver_phase2.rs          CHANGE: same as solver_phase1.rs
│   ├── envelopes.rs              unchanged
│   ├── results.rs                unchanged
│   ├── types.rs                  unchanged
│   └── tests/                    unchanged (existing); new n=48 test added
│       ├── mod.rs                CHANGE: expose new n=48 profile fixture
│       ├── basic.rs              unchanged
│       ├── heater.rs             CHANGE: fill existing todo!() stubs once trait implemented
│       ├── planner.rs            CHANGE: add n=48 regression test
│       ├── pv.rs                 unchanged
│       └── solver.rs             unchanged
├── milp_interactions.rs          CHANGE: remove use crate::assets::battery/ev/heater imports;
│                                         import BatteryMilpVars/EvMilpVars/HeaterMilpVars from
│                                         crate::controller::milp_planner::asset_port instead
└── mod.rs                        unchanged

VEN/src/assets/
├── mod.rs                        CHANGE: build_milp_context() returns Box<dyn AssetMilpContext> instead of AnyMilpContext; AnyMilpContext retained as internal construction helper (not pub outside module)
├── battery.rs                    CHANGE: remove struct defs (moved to asset_port.rs); add pub use re-exports; implement AssetMilpContext; add #[cfg(test)] block
├── ev.rs                         CHANGE: remove struct defs (moved to asset_port.rs); add pub use re-exports; implement AssetMilpContext; add #[cfg(test)] block
└── heater.rs                     CHANGE: remove struct defs (moved to asset_port.rs); add pub use re-exports; implement AssetMilpContext; fill todo!() stubs

VEN/src/controller/milp_planner/tests/profiles/
└── test48.yaml                   NEW: n=48 (24 h, 1800 s steps) regression profile with battery + EV + heater + PV
```

**Structure Decision**: Single Rust workspace, VEN backend only. No new top-level directories. `asset_port.rs` is added inside the existing `controller/milp_planner/` sub-module.
