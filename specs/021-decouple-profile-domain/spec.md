# Feature Specification: Decouple PROFILE from Domain (Phase 4)

**Feature Branch**: `021-decouple-profile-domain`
**Created**: 2026-05-11
**Status**: Draft
**Phase**: 4 of VEN Backend Architecture Refactoring
**Addresses**: Architectural breach AB-04 (PROFILE imported by domain ring) and partially AB-06 (excluded from this phase — `routes/hems.rs` addressed in Phase 6)

## Background

The VEN HEMS domain logic — planning, asset modelling, dispatch, simulation — currently reads its configuration directly from the YAML profile loaded at startup. This means that every domain unit test must either load a real YAML file or construct a full profile fixture. Adding a field to the YAML schema can require touching domain files. The domain is structurally coupled to the configuration format.

This phase cuts that coupling. The YAML profile stays in the infrastructure ring where it belongs. The domain ring — entities, assets, controller, simulator — receives only plain, typed parameter values that contain exactly the numbers it needs, assembled by the application layer before the domain starts.

After this phase, every domain component is independently testable with a few lines of construction code and no YAML at all.

## Readiness Review — Phases 1–3

Before implementing Phase 4, the following invariants from prior phases must hold. Each can be verified with a targeted search on the `refactoring_phase_3` branch.

| ID    | Phase | Invariant | Verification |
|-------|-------|-----------|--------------|
| RR-01 | 1     | `loops.rs` has been replaced by `tasks/` — no god module | `VEN/src/tasks/` directory present; `VEN/src/loops.rs` absent |
| RR-02 | 1     | Each `spawn_*` function lives in its own task file | `tasks/` contains: `sim_tick/`, `planning.rs`, `obligation.rs`, `poll_events.rs`, `poll_programs.rs`, `poll_reports.rs`, `state_persist.rs` |
| RR-03 | 2     | `SimulatorPort` trait is defined in the controller boundary | `VEN/src/controller/simulator_port.rs` present, contains `pub trait SimulatorPort` |
| RR-04 | 2     | Domain controller modules accept `SimSnapshot`, not direct `SimState` imports | `grep "use crate::simulator" VEN/src/controller/` returns zero matches in production modules |
| RR-05 | 3     | `milp_planner` has been split into a sub-module directory | `VEN/src/controller/milp_planner/` is a directory with `inputs.rs`, `types.rs`, `results.rs`, `envelopes.rs`, `solver_phase1.rs`, `solver_phase2.rs` |
| RR-06 | 3     | No concrete asset-type imports remain in `milp_planner` production code | `grep -r "use crate::assets::" VEN/src/controller/milp_planner/` returns zero matches outside `#[cfg(test)]` blocks |
| RR-07 | 3     | `AssetMilpContext` trait is implemented by each optimisable asset | `grep -rn "impl.*AssetMilpContext" VEN/src/assets/` matches `battery.rs`, `ev.rs`, `heater.rs` |

**Current status (assessed 2026-05-11 on `refactoring_phase_3` branch):**

- RR-01 ✅ `loops.rs` absent; `tasks/` directory present
- RR-02 ✅ All expected task files present including `tasks/sim_tick/` sub-module
- RR-03 ✅ `controller/simulator_port.rs` present
- RR-04 ✅ Controller modules import `SimSnapshot` not `SimState`
- RR-05 ✅ `milp_planner/` is a directory with all expected submodules
- RR-06 ✅ No concrete asset imports in `milp_planner` production code
- RR-07 ✅ `AssetMilpContext` implemented in `battery.rs`, `ev.rs`, `heater.rs`

**Conclusion: Codebase is ready for Phase 4.**

## Adjustment Tasks

One structural issue must be resolved before or at the start of Phase 4 implementation. It is not a prerequisite failure but a design decision that shapes the approach:

### ADJ-01 — Relocate `PlannerObjective` to the domain ring

`PlannerObjective` (the enum controlling which optimisation objective the planner uses — `MinCost`, `MaxRevenue`, `MinGhg`, `Custom`) is currently defined in `profile.rs` in the infrastructure ring. However, it is used throughout domain logic: `entities/plan.rs`, `controller/dispatcher.rs`, `controller/absorber.rs`, and multiple `controller/milp_planner/` submodules reference it as a core domain value.

`PlannerObjective` is not a configuration format detail — it is a domain concept that controls planning behaviour at runtime. It belongs in the domain ring.

**Resolution**: Move `PlannerObjective` to the domain ring (e.g. `entities/planner_params.rs`) before cleaning up profile imports in domain files. The profile layer then reads the YAML value and maps it to the domain type at startup. This eliminates all `use crate::profile::PlannerObjective` imports from domain code as a direct consequence.

This adjustment task is the first step of implementation — it unblocks all subsequent profile removals in `dispatcher.rs`, `absorber.rs`, `plan.rs`, and the `milp_planner/` submodules.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Test Domain Logic Without Loading YAML (Priority: P1)

A developer writing or debugging a unit test for any domain component — a planning constraint, an asset model, the absorber, the dispatcher — constructs the test by passing a handful of typed parameter values directly. There is no YAML file to load, no profile path to set, no fixture directory to manage. The test compiles and runs in milliseconds.

**Why this priority**: This is the core payoff of Phase 4. The architecture plan identifies domain tests as having zero coverage today precisely because they cannot be written without a profile fixture. Once YAML is out of the domain ring, the full test surface described in the architecture plan's Layer 1 (domain tests) becomes reachable. Everything else in this phase depends on this property being established.

**Independent Test**: Can be tested by writing a single unit test for any one domain component — e.g. asset constraint generation or planner weight building — that constructs parameters inline with no file I/O. If it compiles and passes, this story is satisfied.

**Acceptance Scenarios**:

1. **Given** a domain unit test for any asset model, planner component, absorber, or dispatcher function, **When** the test is written using only inline parameter construction, **Then** the test compiles and passes with zero file system access.
2. **Given** the existing domain tests that currently load a YAML profile fixture, **When** Phase 4 is complete, **Then** those tests have been rewritten to use inline construction — the YAML fixture load is gone.
3. **Given** a new test written after Phase 4, **When** the developer wants to exercise an edge case (e.g. a battery with very low capacity), **Then** they change one number in the parameter struct, not a YAML file.

---

### User Story 2 - Add a New YAML Config Field Without Touching Domain Code (Priority: P2)

A developer adding a new tunable parameter to the VEN (e.g. a new absorber sensitivity threshold, a new planner penalty weight, a new asset characteristic) edits the YAML schema and the application-layer assembly code. They do not open any file in `entities/`, `assets/`, `controller/`, or `simulator/`. Domain files are untouched.

**Why this priority**: This is the maintainability payoff. The current structure causes profile change requests to ripple into domain files — a violation of the dependency rule. Closing this coupling makes future configuration evolution cheap. It is lower priority than P1 because P1 is a prerequisite (the same structural change that enables P1 also enables P2).

**Independent Test**: Can be tested by adding a dummy config field to the YAML schema and confirming that zero domain files need editing to compile after the change.

**Acceptance Scenarios**:

1. **Given** the application layer assembles all domain parameter structs from the profile at startup, **When** a new YAML config field is added to an existing config section, **Then** only the YAML schema struct and the application-layer assembly function require changes.
2. **Given** domain parameter structs that carry exactly the values the domain needs, **When** the YAML field names or structure are changed, **Then** the domain structs are unchanged — only the mapping from YAML to domain params is updated.
3. **Given** an asset file (`battery.rs`, `ev.rs`, `heater.rs`, `pv.rs`, `base_load.rs`), **When** a developer opens it, **Then** it contains no YAML schema types, no `serde` deserialization attributes for config, and no reference to any profile module.

---

### User Story 3 - VEN Runtime Behaviour Unchanged After Phase 4 (Priority: P3)

An operator running the VEN after Phase 4 is deployed observes identical planning decisions, asset setpoints, absorber behaviour, and simulator physics to those produced before Phase 4. The refactoring is purely structural — zero behaviour change.

**Why this priority**: Correctness preservation is a hard constraint for every phase in this plan. It is listed P3 not because it is less important, but because the prior two stories drive the structural changes that this story verifies. The BDD suite is the safety net.

**Independent Test**: The existing BDD suite, run against the refactored VEN image, must remain fully green with no scenario changes or tag exclusions.

**Acceptance Scenarios**:

1. **Given** the existing BDD suite passing on the Phase 3 codebase, **When** Phase 4 changes are deployed, **Then** all BDD scenarios continue to pass.
2. **Given** a planning cycle triggered by a deviation event, **When** the planner runs after Phase 4, **Then** the plan's slot-by-slot allocations are numerically identical (within floating-point tolerance) to those produced on the Phase 3 codebase for the same inputs.
3. **Given** the absorber running under a grid deviation, **When** Phase 4 is deployed, **Then** the absorber's correction overlays and residual values are identical to those produced before Phase 4.
4. **Given** the simulator physics tick running with a site profile containing battery, EV, heater, PV, and base load, **When** Phase 4 is deployed, **Then** the per-asset state transitions are identical to those produced before Phase 4.

---

### Edge Cases

- What happens if the application layer fails to construct a required parameter struct from the profile (e.g. a required field is missing in the YAML)? The failure must occur at startup, not silently at runtime — the system must refuse to start with an informative error, identical to current behaviour.
- What happens if a domain parameter struct is constructed with out-of-range values (e.g. negative battery capacity)? Domain code should behave consistently with today's behaviour — validation responsibility remains with the application layer assembly, not the domain struct.
- What happens when the planner receives a `PlannerObjective` value at runtime (via an override from an OpenADR event) that differs from the profile default? The domain must accept the value as a pure parameter — the mechanism for overriding the objective at runtime must continue to work after `PlannerObjective` moves to the domain ring.
- What happens in tests that currently rely on `BatteryConfig::default()` or `PlannerConfig::default()` profile-fixture helpers? These tests must be updated to use the new domain parameter struct defaults instead.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The `entities/`, `assets/`, `controller/`, and `simulator/` modules MUST contain zero `use crate::profile` imports in production code after Phase 4. (`routes/hems.rs` is explicitly excluded — it is addressed in Phase 6.)
- **FR-002**: Each domain component (asset model, planner, absorber, dispatcher, simulator) MUST receive the configuration values it needs as typed parameter structs passed at construction time, not by reading from a profile object.
- **FR-003**: `PlannerObjective` MUST reside in the domain ring (e.g. `entities/`) so that all modules currently importing it from `profile.rs` can do so without a profile dependency.
- **FR-004**: Domain parameter structs MUST carry exactly the numeric and boolean values the domain needs — they MUST NOT re-expose YAML schema types, `serde` attributes, or config-format concerns.
- **FR-005**: Domain parameter structs MUST provide `Default` implementations that encode sensible in-code defaults, so that unit tests can construct a baseline struct with one line and override only the fields relevant to the test.
- **FR-006**: The application layer (startup / `main.rs` equivalent) MUST be the sole site where a `Profile` object is read and domain parameter structs are assembled from it.
- **FR-007**: All existing domain unit tests that currently load a YAML profile fixture MUST be rewritten to use inline parameter struct construction — no file I/O in domain tests.
- **FR-008**: At least one new unit test per asset (`battery.rs`, `ev.rs`, `heater.rs`, `pv.rs`, `base_load.rs`) MUST exercise the asset's core domain logic using direct parameter struct construction.
- **FR-009**: The planner's existing unit test suite MUST continue to pass after the profile fixture loading in test setup is replaced with inline parameter construction.
- **FR-010**: The BDD suite MUST remain fully green after Phase 4 — no scenario changes, no tag exclusions.
- **FR-011**: The `simulator/mod.rs` `from_profile()` constructor MUST be replaced by a constructor that accepts assembled domain parameter structs rather than a raw `Profile` reference.
- **FR-012**: `simulator/persist.rs` MUST NOT import `Profile` — any values it currently reads from the profile at persist time must be supplied as parameters or eliminated.

### Key Entities

- **Domain Parameter Structs**: Plain typed structs carrying exactly the values each domain component needs — e.g. `BatteryParams`, `EvParams`, `HeaterParams`, `PvParams`, `BaseLoadParams`, `PlannerParams`, `AbsorberParams`. These live in the domain ring and have no knowledge of the YAML format. Each provides a `Default` implementation.
- **`PlannerObjective`**: The domain enum controlling which optimisation objective the planner uses (`MinCost`, `MaxRevenue`, `MinGhg`, `Custom`). After Phase 4 it lives in the domain ring, not in `profile.rs`.
- **Application-Layer Assembly**: The code in `main.rs` (or an equivalent startup module) that reads a `Profile` and constructs the domain parameter structs. This is the only place in the codebase where `Profile` is transformed into domain types.
- **`Profile`**: The YAML-deserialisable configuration struct. After Phase 4 it remains in the infrastructure ring and is never imported by domain code.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: `grep -r "use crate::profile" VEN/src/entities VEN/src/assets VEN/src/controller VEN/src/simulator` returns zero matches.
- **SC-002**: Every asset file (`battery.rs`, `ev.rs`, `heater.rs`, `pv.rs`, `base_load.rs`) has at least one unit test that constructs the asset using an inline parameter struct and requires no file I/O.
- **SC-003**: All existing domain tests (`controller/milp_planner/tests/`, `controller/absorber.rs`, `controller/dispatcher.rs`) pass after their profile fixture loading is replaced with inline parameter construction — test count does not decrease.
- **SC-004**: All BDD scenarios pass on the first post-Phase-4 full suite run with no modifications to scenario files.
- **SC-005**: `PlannerObjective` is importable from the domain ring with no `profile` module in the import path.

## Assumptions

- `routes/hems.rs` PROFILE imports (`R_HEMS → PROFILE`) are **out of scope** for this phase — they are addressed in Phase 6 as described in the architecture plan.
- `PlannerObjective` values injected via OpenADR event overrides at runtime continue to flow through the existing watch-channel mechanism — the relocation of `PlannerObjective` to the domain ring does not change the runtime injection path, only the type's home module.
- No logic changes are made in any file touched during this phase. Phase 4 is a purely structural extraction.
- The `Profile` struct itself is not deleted or modified — it remains the YAML deserialisation target. Only its reach into the domain ring is cut.
- `simulator/persist.rs` currently imports `Profile` — inspection during implementation may reveal that the import is unused or can be trivially substituted with a stored copy of a primitive value already available in `SimState`. If not, a minimal domain param struct is introduced.
