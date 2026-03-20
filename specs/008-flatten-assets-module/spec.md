# Feature Specification: Flatten Assets Module

**Feature Branch**: `008-flatten-assets-module`
**Created**: 2026-03-20
**Status**: Draft
**Input**: User description: "Flatten simulator/assets/ into a top-level assets/ module (RF-02)"

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Developer navigates to an asset module (Priority: P1)

A developer looking for the PV physics model, forecast logic, or simulation parameters opens `VEN/src/assets/pv/` and finds everything in one place. No detour through a `simulator/` wrapper.

**Why this priority**: This is the core motivation of the refactor — remove the misleading `simulator/` parent directory that implies simulation is a global concern rather than an asset-level one.

**Independent Test**: Verify `VEN/src/assets/` exists, each sub-module compiles, and the old `simulator/assets/` path is absent.

**Acceptance Scenarios**:

1. **Given** the repository is checked out, **When** a developer opens `VEN/src/assets/`, **Then** they find one sub-directory per asset type (`pv`, `battery`, `ev`, `heater`, `base_load`) plus `mod.rs`.
2. **Given** the old path `VEN/src/simulator/assets/` existed, **When** the refactor is complete, **Then** that path no longer exists in the source tree.
3. **Given** the codebase compiled before the move, **When** the refactor is complete, **Then** `cargo build` produces zero errors and zero new warnings.

---

### User Story 2 — All existing tests continue to pass (Priority: P2)

After the move, all BDD integration tests and cargo unit tests pass without modification to test logic — only import paths inside source files are updated.

**Why this priority**: This is a pure structural refactor. Any test breakage is a regression. Passing tests confirm the public API surface is unchanged.

**Independent Test**: Run `cargo test --workspace` and the full BDD suite; both must exit 0.

**Acceptance Scenarios**:

1. **Given** 895 BDD integration steps pass on the pre-refactor baseline, **When** this refactor is applied, **Then** all 895 steps still pass.
2. **Given** `cargo test --workspace` passes before the move, **When** all source paths are updated, **Then** `cargo test --workspace` still passes with zero failures.

---

### Edge Cases

- What if a `use crate::simulator::assets::...` import exists in a non-obvious location (macro, test helper, or generated code)? All `use` paths must be found and updated.
- What if `simulator/assets/mod.rs` re-exported symbols consumed by `simulator/mod.rs`? Those re-exports must be replicated at the new location or call sites updated.
- What if `simulator/mod.rs` has other sub-modules beyond `assets/` (e.g., `energy`, `persist`, `power_model`)? Only the `assets/` sub-tree moves; those modules remain in `simulator/`.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The directory `VEN/src/assets/` MUST exist and contain one sub-directory per asset type (`pv`, `battery`, `ev`, `heater`, `base_load`) and a `mod.rs`.
- **FR-002**: `VEN/src/assets/mod.rs` MUST declare `AssetInterface`, `AssetEntry`, and the `Vec<AssetEntry>`-based `SimState` — the types currently in `simulator/assets/mod.rs`.
- **FR-003**: Each asset sub-module MUST contain all physics, forecast, simulation state, and `/sim` parameter types currently under `simulator/assets/<asset>/`.
- **FR-004**: The path `VEN/src/simulator/assets/` MUST NOT exist after the refactor is complete.
- **FR-005**: All `use` / `mod` references to `simulator::assets` across the codebase MUST be updated to `assets` (or `crate::assets`).
- **FR-006**: The remaining contents of `VEN/src/simulator/` (sub-modules other than `assets/`, and `simulator/mod.rs` itself) MUST remain untouched and compile without errors.
- **FR-007**: No public API changes (HTTP routes, response field names, status codes) are permitted as part of this refactor.

### Key Entities

- **AssetInterface trait**: Defined in `assets/mod.rs`; implemented by each asset type.
- **AssetEntry**: Wraps an `AssetState` enum variant plus runtime fields; lives in `assets/mod.rs`.
- **Per-asset modules** (`pv`, `battery`, `ev`, `heater`, `base_load`): Each owns its physics model, state struct, and sim config types.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: `cargo build` completes with zero errors and zero new warnings after the move.
- **SC-002**: All cargo unit tests pass (`cargo test --workspace` exits 0, same count as before the move).
- **SC-003**: All BDD integration tests pass (895 steps, 0 failures — matching the pre-refactor baseline).
- **SC-004**: The path `VEN/src/simulator/assets/` is absent from the source tree.
- **SC-005**: The path `VEN/src/assets/` is present and contains exactly five asset sub-directories plus `mod.rs`.
- **SC-006**: No HTTP endpoint contracts are changed; existing integration test assertions require no updates to their assertion logic.

## Assumptions

- The refactor is a file-system and `mod`/`use` path rename only. No logic, types, or trait implementations are modified.
- `VEN/src/simulator/` retains its other sub-modules after the move; only `assets/` is relocated.
- The current branch (`007-asset-forecast-past`) is the baseline: 895 BDD steps and `cargo test` pass there before this work begins.
