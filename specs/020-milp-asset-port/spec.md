# Feature Specification: MILP Asset Port — Decouple Planner from Concrete Asset Types

**Feature Branch**: `020-milp-asset-port`  
**Created**: 2026-05-10  
**Status**: Draft  
**Phase**: 3 of VEN Backend Architecture Refactoring  
**Addresses**: Architectural breach AB-02 (direct planner-to-asset-type coupling)

## Background

The VEN HEMS planner currently has a hard dependency on each concrete optimisable asset type (Battery, EV Charger, Heater). Both the planner's constraint builder and the cross-asset interaction module name these asset types explicitly. Adding a new optimisable asset — for example, a hot-water tank or a flexible industrial load — requires editing both planner files even though the planner logic itself does not change.

This phase introduces a single abstract contract that every optimisable asset must satisfy, making the planner open for extension without modification.

## Clarifications

### Session 2026-05-10

- Q: How does the cross-asset interaction (BatEvCoexistInteraction) determine whether a battery AND EV are both present when assets are accessed via the trait? → A: Co-presence is resolved via pool-slot presence: `build_interactions()` checks `pool.bat.is_some() && pool.ev.is_some()`. Each asset's `declare_vars_into_pool()` populates exactly its own named pool slot, so a populated slot implies that asset kind is present — without any concrete type imports. `asset_kind()` is used for dispatching variable declaration to the correct pool slot and for structured logging, not for applicability matching in the interaction module.
- Q: Which test profile defines the regression baseline for SC-005? → A: Both — the existing n=24 (2 h, 300 s steps) fast check AND a n=48 (24 h, 1800 s steps) medium check that covers a full PV cycle and storage assets.
- Q: FR-010 says `AnyMilpContext` "MUST be replaced or superseded"; Assumptions says it "may be retained internally." Which governs? → A: Assumptions governs — `AnyMilpContext` may survive as an internal construction helper inside `assets/` provided it is never imported by any planner or interaction module.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Add a New Optimisable Asset Without Touching the Planner (Priority: P1)

A developer adding a new optimisable asset type to the VEN (e.g. a hot-water tank, a flexible load, or a vehicle-to-grid inverter) implements the asset's planning contract once in the asset's own file. The developer does not open, edit, or retest either the constraint-building module or the cross-asset interaction module. The new asset participates in planning automatically.

**Why this priority**: This is the core value of the phase. The entire motivation for Phase 3 is to eliminate the fan-out edit burden where every new asset type requires changes to two central planner files. Until this story is satisfied, the architectural breach AB-02 remains open.

**Independent Test**: Can be tested by writing a minimal stub asset that implements the planning contract and confirming that the planner produces a valid plan including that asset — without any changes to the planner or interaction modules.

**Acceptance Scenarios**:

1. **Given** a new asset type whose planning contract is fully implemented, **When** the planner is run with that asset present in the site profile, **Then** the planner incorporates the asset's variables and constraints and produces a valid, feasible plan.
2. **Given** the planner source files and the interaction module source file, **When** a new asset type is added, **Then** neither file requires any edits.
3. **Given** an asset type that declares itself non-MILP-capable (e.g. PV, base load, grid), **When** the planner encounters it, **Then** the asset is silently skipped — no error, no panic, no constraint violation.

---

### User Story 2 - Test Planner Logic in Isolation Per Asset (Priority: P2)

A developer writing or debugging unit tests for the planner can test constraint-building and solution extraction for a single asset type without constructing a full site profile, loading YAML, or running the solver end-to-end. Each asset's planning contract implementation is independently exercisable with a known input and a known expected output.

**Why this priority**: Phase 3 unlocks a new layer of the test pyramid identified in the architecture plan (`inputs::build_milp_inputs()`, `phase1::build_constraints()`, `phase2::build_constraints()`, `translate::translate_solution()` — all previously untestable without a full profile fixture). This story captures that test-surface unlock.

**Independent Test**: Can be tested by writing unit tests for each asset's planning contract implementation that verify variable count and constraint count against hand-computed expected values — with no solver invocation. *(Solution-reading / setpoint extraction is excluded from the trait in this phase; see FR-003.)*

**Acceptance Scenarios**:

1. **Given** a battery asset planning contract seeded with known parameters (capacity, SoC, charge/discharge limits), **When** the variable-declaration step is called, **Then** the correct number and types of LP variables are registered.
2. **Given** a battery asset planning contract seeded with known capacity, initial SoC, and power limits, **When** the constraint-building step is called, **Then** the generated constraints include SoC-continuity across adjacent slots, charge/discharge capacity bounds, and power limits — verified by constraint count and bound values matching hand-computed expectations.
3. **Given** an EV charger asset planning contract with a departure deadline in the future, **When** the constraint-building step is called, **Then** the energy-by-deadline constraint is present and correctly bounded.

---

### User Story 3 - Planner Behaviour Unchanged After Refactoring (Priority: P3)

An operator running the VEN after Phase 3 is deployed observes identical planning decisions, energy allocations, and plan summaries to those produced before Phase 3. The refactoring is purely structural — zero behaviour change.

**Why this priority**: Correctness preservation is a hard requirement for every refactoring phase in this plan. The BDD suite (232 scenarios) is the safety net. This story ensures that the abstraction layer introduced in Phase 3 is transparent at runtime.

**Independent Test**: The existing BDD suite, run against the refactored VEN image, must remain fully green with no scenario changes.

**Acceptance Scenarios**:

1. **Given** the existing BDD suite passing on the Phase 2 codebase, **When** the Phase 3 changes are deployed, **Then** all 232 BDD scenarios continue to pass.
2. **Given** a planning cycle triggered by a deviation event, **When** the planner runs with the Phase 3 asset abstraction, **Then** the resulting plan's slot-by-slot allocations are numerically identical (within floating-point tolerance) to those produced by the Phase 2 planner for the same inputs.
3. **Given** a site profile with battery, EV charger, and heater, **When** the planner runs after Phase 3, **Then** all three assets appear in the plan's allocation breakdown as before.

---

### Edge Cases

- What happens when no optimisable assets are present in the profile (PV-only or base-load-only site)? The planner must still produce a valid plan with only grid and base-load constraints.
- What happens when an asset's planning contract is present but the asset is in a state that makes it infeasible to include (e.g. EV unplugged, heater at target temperature)? The contract implementation must correctly suppress or trivialise its variables and constraints rather than injecting infeasible bounds.
- What happens when two assets of the same type are present (e.g. two batteries)? Each must register distinct LP variables — no variable aliasing.
- What happens when the solver finds no feasible solution after Phase 3? The fallback plan path must still be reached and must produce a valid (non-panicking) result, identical to the pre-Phase-3 fallback behaviour.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The planner's constraint-builder and the cross-asset interaction module MUST accept optimisable assets exclusively through the abstract planning contract — no direct imports of concrete Battery, EV Charger, or Heater types.
- **FR-002**: Each optimisable asset (Battery, EV Charger, Heater) MUST implement the planning contract in its own asset file, with no changes required to the planner or interaction module files when a new asset implements the contract.
- **FR-003**: The planning contract MUST cover the planning-side lifecycle of an asset's participation in one planning cycle: LP variable declaration, constraint generation, and asset-kind identification via an `asset_kind()` method returning a well-known discriminant. *(Setpoint extraction / solution-reading is handled by `results.rs` via the `MilpVarPool` typed slots, which already reside in the controller boundary and are architecturally compliant with the constitution. Adding a solution-reading method to the trait is out of scope for this phase; it may be revisited in a dedicated refactoring pass once the port stabilises.)*
- **FR-004**: The Grid asset MUST NOT implement the planning contract — it is the reference bus, not an optimisable asset, and has no decision variables.
- **FR-005**: The PV inverter and base-load assets MUST NOT implement the planning contract — they are treated as fixed forecasts by the planner, not controllable assets.
- **FR-006**: The planner MUST produce numerically identical results (within floating-point tolerance) to the pre-Phase-3 implementation for any given set of inputs.
- **FR-007**: The existing BDD suite MUST remain fully green after Phase 3 changes are merged — no scenario modifications, no tag exclusions.
- **FR-008**: Each asset's planning contract implementation MUST be unit-testable in isolation, with no dependency on a YAML profile, a running simulator, or a real LP solver.
- **FR-009**: The cross-asset interaction infrastructure (battery-EV coexistence penalty, McCormick envelope) MUST continue to function correctly after the concrete asset type references are replaced with the abstract contract. Applicability checks (e.g. "are both a battery and an EV present?") are resolved via pool-slot presence — `build_interactions()` checks `pool.bat.is_some() && pool.ev.is_some()`. This is architecturally equivalent to querying by kind: each asset's `declare_vars_into_pool()` populates exactly its own named pool slot, so a populated slot implies that asset kind is present. No direct imports of `Battery`, `EvCharger`, or `Heater` are permitted in the interaction module.
- **FR-010**: The planner constraint-builder and the cross-asset interaction module MUST NOT import or match on `AnyMilpContext` or any concrete asset variant. The `AnyMilpContext` enum in `assets/mod.rs` may be retained as an internal construction helper within the `assets/` module boundary — it must not cross that boundary into the planner or interaction module.

### Key Entities

- **Planning Contract** (`AssetMilpContext` trait): The abstract interface an optimisable asset must satisfy to participate in planning. Covers variable declaration, constraint generation, solution reading, and asset-kind identification (`asset_kind()` — a well-known discriminant used by cross-asset interactions to detect asset co-presence without concrete type imports). Lives in the controller layer, not in the assets layer.
- **Optimisable Asset Implementation**: The concrete planning contract implementation for each asset (Battery, EV Charger, Heater). Lives in each asset's own source file alongside existing physics and simulation logic.
- **LP Variable Pool**: The shared container that collects all LP variable handles from all assets for one planning cycle. After Phase 3, assets register themselves into this pool via the contract rather than being registered by name from the planner.
- **Cross-Asset Interaction**: The infrastructure that models LP constraints coupling two or more assets (e.g. battery-EV coexistence). After Phase 3, it queries the pool via the contract rather than by concrete asset type.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A new optimisable asset can be added to the planner by implementing the planning contract in a single file — zero edits to the planner constraint-builder or cross-asset interaction module are required. *(This phase verifies the criterion structurally: the constitution invariant grep in T014 confirms no concrete asset imports remain in the planner. Functional proof — adding a real stub asset and running the full planner with it — is explicitly deferred to the next new-asset feature branch, which will serve as the first integration validation of the boundary.)*
- **SC-002**: All 232 existing BDD scenarios pass against the Phase 3 VEN image with no modifications to any scenario or step file.
- **SC-003**: At least one unit test per concrete planning contract implementation (Battery, EV Charger, Heater) is present and passes in the Rust test suite without loading any YAML profile or invoking the LP solver.
- **SC-004**: Neither the planner constraint-builder module nor the cross-asset interaction module contains a named import of `Battery`, `EvCharger`, or `Heater` after Phase 3 is complete.
- **SC-005**: The planner produces slot-by-slot allocations that are numerically identical (absolute difference ≤ 1 × 10⁻⁶ kW per slot) to the pre-Phase-3 baseline for the same inputs, verified by two regression tests in the Rust test suite: (a) a fast n=24 check (2 h horizon, 300 s steps) and (b) a medium n=48 check (24 h horizon, 1800 s steps) that covers a full PV generation cycle and storage assets with overnight discharge.

## Assumptions

- Phase 1 (tasks/ split) and Phase 2 (SimulatorPort trait) are already merged and green on the `refactoring_phase_3` branch. This phase builds on that foundation.
- The `milp_planner` sub-module split (inputs, types, solver phases, envelopes, results, tests) is already in place on this branch. Phase 3 refines the inter-module dependency graph within that structure.
- The `AnyMilpContext` enum currently in `assets/mod.rs` may be retained as an internal construction helper within the `assets/` module boundary. It must not be imported by any planner or interaction module — the planner receives only `Box<dyn AssetMilpContext>` trait objects.
- No new asset types are being added in this phase — only the abstraction boundary is introduced. The first new asset type will validate the boundary in a subsequent feature.
- The n=24 test profile (2 h, 300 s steps) already exists in `milp_planner/tests/`. A new n=48 test profile (24 h, 1800 s steps) will be introduced in this phase specifically to serve the SC-005 medium regression baseline; it must include battery, EV, heater, and PV to exercise the full asset mix across a day.
- The `milp_interactions.rs` `MilpVarPool` retains its named, typed fields (`bat`, `ev`, `heater`), each populated by the corresponding asset's `declare_vars_into_pool()` call. No `Vec` of trait objects is introduced into the pool — the named-field design preserves full compatibility with `results.rs` and `build_interactions()` while satisfying the port contract. *(Resolved: this was previously flagged as "left to the planning phase"; tasks.md T004–T006 encode the named-field approach.)*
