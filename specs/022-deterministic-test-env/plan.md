# Implementation Plan: Deterministic Test Environment for MILP-Backed BDD Tests

**Branch**: `022-deterministic-test-env` | **Date**: 2026-05-12 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `specs/022-deterministic-test-env/spec.md`

## Summary

MILP-backed BDD scenarios are non-deterministic because the 24-hour PV forecast is derived from the real system clock. Injecting `pv_irradiance=0.0` zeros the current physics tick, but the planning horizon still sees the natural sin-model curve; at solar-prep hours the battery is pre-discharged, leaving insufficient headroom for deviation-absorber assertions.

The fix is a single new optional field `pv_plan_kw` on `SimInjectState`. When set, it replaces the irradiance-derived per-slot PV value with a constant across the entire planning horizon, making every MILP solve clock-independent. The field is threaded from the inject endpoint through `planning.rs` into `build_milp_inputs`. It does not trigger a replan (consistent with `base_load_kw`).

The change is adopted suite-wide: every MILP-backed BDD feature file that makes battery-dispatch assertions receives `pv_plan_kw=0.0` in its Background, eliminating time-of-day non-determinism across the full test suite. One new BDD step `I set pv plan forecast to {kw:f} kW` provides the composable vocabulary.

## Technical Context

**Language/Version**: Rust stable 2021 edition
**Primary Dependencies**: tokio (async runtime), axum (HTTP), serde/serde_json (inject body), good_lp / HiGHS (MILP solver — unchanged)
**Storage**: N/A — no persistence schema changes; `pv_plan_kw` is in-memory only
**Testing**: `cargo test --workspace` (unit), `behave` BDD via Docker (integration)
**Target Platform**: Linux ARM64 (Raspberry Pi 4), Docker Compose v2
**Project Type**: Targeted field addition to existing Rust binary (`VEN/`) + BDD step + feature file updates
**Performance Goals**: No regressions — MILP solve time and sim tick latency unchanged
**Constraints**: Zero behaviour change in production (field is opt-in, absent ⇒ existing model runs); no new crate dependencies
**Scale/Scope**: 4 Rust files touched, 1 new BDD step, ~1–5 feature file Backgrounds updated after audit

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Gate | Status |
|-----------|------|--------|
| I — OpenADR Spec Fidelity | No OpenADR field names involved — `pv_plan_kw` is an internal inject override | ✅ N/A |
| II — BDD-First Testing | Target scenario (`deviation_absorber.feature:149`) is already written and tagged `@wip` (red); infrastructure must be added to make it green. New BDD step written before Rust changes. | ✅ Satisfied |
| III — Upstream Compatibility | VEN is not part of the `openleadr-rs` submodule; no upstream PR required | ✅ N/A |
| IV — Lean Architecture | One new `Option<f64>` field, one new macro call in `merge_inject`, one branch check in `build_milp_inputs`, one new parameter on `run_planner`. No new types, no new layers. | ✅ Satisfied |
| V — Infrastructure Parity | No new Docker config; build and test via existing Compose stack on Pi4 | ✅ Satisfied |
| VI — VEN Hexagonal Architecture | `pv_plan_kw` lives in `SimInjectState` (infrastructure ring). The domain ring (`milp_planner/inputs.rs`) receives it as a plain `Option<f64>` parameter — no import of `SimInjectState` in the domain. Dependency rule preserved. | ✅ Satisfied |
| VI — Line limit (500 lines) | `state.rs`: adds 1 field. `routes/sim.rs`: adds 2 lines. `inputs.rs`: adds 1 parameter + ~5 lines. All well within limits. | ✅ No violations |

**Constitution Check: PASSED — no violations, no Complexity Tracking entries required.**

## Project Structure

### Documentation (this feature)

```text
specs/022-deterministic-test-env/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output — SimInjectState extension + step vocabulary
├── quickstart.md        # Phase 1 output — verification commands
├── contracts/
│   └── sim-inject.md   # Phase 1 output — updated POST /sim/inject payload schema
└── tasks.md             # Phase 2 output (/speckit.tasks — not yet created)
```

### Source Code Layout

```text
VEN/src/
├── state.rs                              # Add pv_plan_kw: Option<f64> to SimInjectState
├── routes/sim.rs                         # Add pv_plan_kw to PostSimInjectBody + merge_inject
│                                         # (NOT in should_replan — same as base_load_kw)
├── tasks/planning.rs                     # Pass inject_snap.pv_plan_kw to run_planner
└── controller/milp_planner/
    ├── mod.rs                            # Add pv_forecast_override: Option<f64> to run_planner
    └── inputs.rs                         # Check pv_forecast_override first in p_pv loop

tests/features/
├── steps/
│   └── phase_a_physics_steps.py         # New step: I set pv plan forecast to {kw:f} kW
└── deviation_absorber.feature            # Background: add I set pv plan forecast to 0.0 kW
                                          # Remove @wip from scenario at line 149
                                          # (+ audit: ven_dispatcher, use_cases, ven_planner, etc.)
```

**Structure Decision**: Single VEN binary — no new modules, no new files. All changes are additive extensions to existing files.

