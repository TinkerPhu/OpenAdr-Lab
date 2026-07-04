# Implementation Plan: Multi-Asset Deviation Absorber with Relay Wear Control

**Branch**: `017-add-deviation-absorber` | **Date**: 2026-05-01 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/017-add-deviation-absorber/spec.md`

## Summary

Implement a two-tier deviation control system for the VEN HEMS controller: **Tier 1** is a real-time multi-asset absorber that corrects grid deviations by adjusting battery, EV, and heater setpoints sequentially within flexibility bounds, with relay wear protection via `min_state_linger_s`. **Tier 2** escalates to MILP replanning only when Tier 1's residual deviation persists above 0.1 kW for 120 seconds (production) or 10 seconds (test). This reduces replanning frequency from ~20s to ~120s, cutting Pi4 CPU load from ~50% to ~5%.

## Technical Context

**Language/Version**: Rust (stable 2021 edition)  
**Primary Dependencies**: tokio (async runtime), axum (HTTP), chrono (timestamps), serde/serde_json (config/serialization)  
**Storage**: N/A — in-memory `AbsorberState` per tick; no schema changes (existing JSON persistence via `state.json`)  
**Testing**: cargo test (unit), behave (BDD integration tests in Docker), Playwright (E2E)  
**Target Platform**: Linux ARM64 (Raspberry Pi 4) — Docker Compose deployment  
**Project Type**: Backend service module — HEMS controller subsystem within VEN backend  
**Performance Goals**: Absorber loop completes in <100ms per tick (1s tick interval), Tier 2 escalation fires ≤ 1× per 120s  
**Constraints**: Pi4 resource-limited (cpus: 1.5, memory: 1500M, CARGO_BUILD_JOBS=4)  
**Scale/Scope**: 3 absorber-eligible assets (battery, EV, heater) per site; 1-3 VEN instances in test; production single-VEN baseline

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

**Status: ✅ PASS** — All applicable principles satisfied.

| Principle | Applicability | Assessment |
|-----------|---------------|------------|
| **I. OpenADR Spec Fidelity** | N/A | Not applicable — feature is VEN HEMS controller, not VTN/OpenADR DTOs |
| **II. BDD-First Testing** | ✅ Required | 4 user stories with acceptance scenarios → BDD feature file required in `tests/features/deviation_absorber.feature` |
| **III. Upstream Compatibility** | N/A | Not applicable — changes are in VEN backend, not openleadr-rs submodule |
| **IV. Lean Architecture** | ✅ Required | Minimal design: replace one function, add `AbsorberState` struct, extend profile schema. No premature abstractions. Justified by concrete need (runtime deviation control). |
| **V. Infrastructure Parity** | ✅ Required | All operations on Pi4 via Docker; no new containers; existing named cargo volumes used; resource constraints (1.5 cpus, 1500M memory, CARGO_BUILD_JOBS=4) respected |

**Complexity Tracking**: None — no Constitution violations.

## Project Structure

### Documentation (this feature)

```text
specs/017-add-deviation-absorber/
├── plan.md              # This file (/speckit.plan command output)
├── research.md          # Phase 0 output (none needed — all unknowns resolved in spec clarification)
├── data-model.md        # Phase 1 output (/speckit.plan command)
├── quickstart.md        # Phase 1 output (/speckit.plan command)
├── contracts/           # Phase 1 output (N/A — no new public API/interfaces)
├── checklists/
│   └── requirements.md   # Spec quality checklist
└── tasks.md             # Phase 2 output (/speckit.tasks command)
```

### Source Code

**VEN Backend** — Rust backend (all changes under `VEN/src/`):

```text
VEN/
├── src/
│   ├── controller/
│   │   ├── absorber.rs          # NEW — AbsorberState, apply_deviation_absorption()
│   │   ├── dispatcher.rs        # MODIFY — add apply_battery_correction_overlay (refactored from existing)
│   │   ├── mod.rs              # MODIFY — add pub mod absorber
│   │   └── ...
│   ├── loops.rs                # MODIFY — replace apply_deviation_correction call, integrate absorber
│   ├── profile.rs              # MODIFY — add AbsorberConfig, AbsorberAssetConfig structs
│   └── ...
├── profiles/
│   ├── test.yaml               # MODIFY — add absorber config section
│   ├── ven-1.yaml              # MODIFY — add absorber config section
│   ├── ven-2.yaml              # MODIFY — add absorber config section
│   └── ven-3.yaml              # MODIFY — add absorber config section
└── ...

tests/
├── features/
│   └── deviation_absorber.feature    # NEW — 6 BDD scenarios
├── steps/
│   └── deviation_absorber_steps.py   # NEW — step definitions
└── ...
```

**Structure Decision**: Backend-only module addition within existing VEN Rust project. No new services, frontends, or databases. Changes are isolated to: (1) new absorber module + integrations in `VEN/src/`, (2) profile schema extension, (3) BDD test scenarios.

## Complexity Tracking

> **Fill ONLY if Constitution Check has violations that must be justified**

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| [e.g., 4th project] | [current need] | [why 3 projects insufficient] |
| [e.g., Repository pattern] | [specific problem] | [why direct DB access insufficient] |
